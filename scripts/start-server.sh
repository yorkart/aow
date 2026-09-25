#!/bin/sh

set -eu

script_dir=$(CDPATH='' cd -P "$(dirname "$0")" && pwd)
repo_root=$(CDPATH='' cd -P "$script_dir/.." && pwd)
runtime_root=${AOW_RUNTIME_ROOT:-${HOME:?HOME must be set}/.local/lib/aow}
releases_dir=$runtime_root/releases
active_dir=$runtime_root/active
runtime_bin_dir=$runtime_root/bin
launcher=$runtime_bin_dir/aow-server
unit_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
unit_name=aow-server.service
unit_source=$repo_root/packaging/systemd/aow-server.service
requested_release=${1:-latest}

if [ "$(uname -s)" = Darwin ]; then
    exec sh "$script_dir/start-launchd.sh" server "$requested_release"
fi

user_manager_available() {
    command -v systemctl >/dev/null 2>&1 \
        && systemctl --user show-environment >/dev/null 2>&1
}

resolve_release() {
    requested=$1
    case "$requested" in
        latest) candidate=$runtime_root/latest ;;
        ''|[!A-Za-z0-9]*|*[!A-Za-z0-9._-]*)
            printf 'error: invalid release id: %s\n' "$requested" >&2
            return 1
            ;;
        *) candidate=$releases_dir/$requested ;;
    esac
    if [ ! -d "$candidate" ]; then
        printf 'error: AoW release is unavailable: %s\n' "$candidate" >&2
        return 1
    fi
    resolved=$(CDPATH='' cd -P "$candidate" && pwd)
    resolved_releases_dir=$(CDPATH='' cd -P "$releases_dir" && pwd)
    case "$resolved" in
        "$resolved_releases_dir"/*) ;;
        *)
            printf 'error: release resolves outside %s: %s\n' "$releases_dir" "$resolved" >&2
            return 1
            ;;
    esac
    if [ ! -x "$resolved/bin/aow-server" ] \
        || [ ! -f "$resolved/frontend/dist/index.html" ]; then
        printf 'error: incomplete AoW server release: %s\n' "$resolved" >&2
        return 1
    fi
    printf '%s\n' "$resolved"
}

install_unit() {
    if [ ! -f "$unit_source" ]; then
        printf 'error: systemd unit template not found: %s\n' "$unit_source" >&2
        return 1
    fi
    mkdir -p "$unit_dir"
    unit_path=$unit_dir/$unit_name
    if [ -d "$unit_path" ] && [ ! -L "$unit_path" ]; then
        printf 'error: service unit destination is a directory: %s\n' "$unit_path" >&2
        return 1
    fi
    unit_tmp=$(mktemp "$unit_dir/.${unit_name}.XXXXXX")
    cp "$unit_source" "$unit_tmp"
    chmod 0600 "$unit_tmp"
    mv -f "$unit_tmp" "$unit_path"
}

set_active_release() {
    release=$1
    release_id=$(basename "$release")
    mkdir -p "$active_dir"
    active_path=$active_dir/server
    if [ -e "$active_path" ] && [ ! -L "$active_path" ]; then
        printf 'error: active server path is not a symlink: %s\n' "$active_path" >&2
        exit 1
    fi
    active_tmp=$active_dir/.server.$$.tmp
    rm -f "$active_tmp"
    ln -s "../releases/$release_id" "$active_tmp"
    replace_symlink "$active_tmp" "$active_path"
}

replace_symlink() {
    source=$1
    destination=$2
    if mv -Tf "$source" "$destination" 2>/dev/null; then
        return
    fi
    rm -f "$destination"
    mv -f "$source" "$destination"
}

restore_active_release() {
    if [ -n "$previous_active" ]; then
        restore_tmp=$active_dir/.server.restore.$$.tmp
        rm -f "$restore_tmp"
        ln -s "$previous_active" "$restore_tmp"
        replace_symlink "$restore_tmp" "$active_dir/server"
    else
        rm -f "$active_dir/server"
    fi
}

if ! user_manager_available; then
    printf '%s\n' 'error: a systemd user manager is required to start aow-server' >&2
    exit 1
fi
if [ ! -x "$launcher" ]; then
    printf '%s\n' 'error: AoW launcher is unavailable; install AoW first' >&2
    exit 1
fi

release=$(resolve_release "$requested_release")
bash "$repo_root/packaging/bin/aow" pin --if-missing
was_active=0
if systemctl --user is-active --quiet "$unit_name"; then
    was_active=1
fi
previous_active=
if [ -L "$active_dir/server" ]; then
    previous_active=$(readlink "$active_dir/server")
fi

set_active_release "$release"
if ! install_unit || ! systemctl --user daemon-reload \
    || ! systemctl --user enable --now "$unit_name"; then
    printf '%s\n' 'error: aow-server service activation failed; restoring the previous active release' >&2
    restore_active_release
    exit 1
fi
if [ "$was_active" -eq 1 ]; then
    if ! systemctl --user restart "$unit_name"; then
        printf '%s\n' 'error: aow-server restart failed; restoring the previous active release' >&2
        restore_active_release
        if [ -n "$previous_active" ]; then
            systemctl --user restart "$unit_name" || :
        fi
        exit 1
    fi
fi

printf 'aow-server is active on release %s\n' "$(basename "$release")"
