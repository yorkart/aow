#!/bin/sh

set -eu

script_dir=$(CDPATH='' cd -P "$(dirname "$0")" && pwd)
repo_root=$(CDPATH='' cd -P "$script_dir/.." && pwd)
if [ "${AOW_HOST_ACTIVATION_DETACHED:-}" != 1 ]; then
    exec node "$script_dir/activate-host.mjs" "$script_dir/start-host-daemon.sh" "$@"
fi
install_root=${AOW_INSTALL_ROOT:-${HOME:?HOME must be set}/.local/lib/aow}
releases_dir=$install_root/releases
active_dir=$install_root/active
install_bin_dir=$install_root/bin
launcher=$install_bin_dir/aow-host-daemon
unit_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
unit_name=aow-host-daemon.service
unit_source=$repo_root/packaging/systemd/aow-host-daemon.service
node_link=$install_root/node
node_marker=$install_root/.aow-host-daemon-node-target
node_stage=
cleanup() {
    if [ -n "$node_stage" ]; then
        rm -f "$node_stage/node" "$node_stage/.aow-host-daemon-node-target"
        rmdir "$node_stage" 2>/dev/null || :
    fi
}
trap cleanup 0
trap 'exit 1' 1 2 15
requested_release=${1:-latest}

if [ "$(uname -s)" = Darwin ]; then
    exec sh "$script_dir/start-launchd.sh" host-daemon "$requested_release"
fi

user_manager_available() {
    command -v systemctl >/dev/null 2>&1 \
        && systemctl --user show-environment >/dev/null 2>&1
}

resolve_release() {
    requested=$1
    case "$requested" in
        latest) candidate=$install_root/latest ;;
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
    if [ ! -x "$resolved/bin/aow-host-daemon" ]; then
        printf 'error: incomplete AoW host-daemon release: %s\n' "$resolved" >&2
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
    active_path=$active_dir/host-daemon
    if [ -e "$active_path" ] && [ ! -L "$active_path" ]; then
        printf 'error: active host-daemon path is not a symlink: %s\n' "$active_path" >&2
        exit 1
    fi
    active_tmp=$active_dir/.host-daemon.$$.tmp
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
        restore_tmp=$active_dir/.host-daemon.restore.$$.tmp
        rm -f "$restore_tmp"
        ln -s "$previous_active" "$restore_tmp"
        replace_symlink "$restore_tmp" "$active_dir/host-daemon"
    else
        rm -f "$active_dir/host-daemon"
    fi
}

install_node_link() {
    if ! command -v node >/dev/null 2>&1; then
        printf '%s\n' 'error: Node.js 20 or newer is required by aow-host-daemon' >&2
        exit 1
    fi
    node_path=$(command -v node)
    case "$node_path" in
        /*) ;;
        *)
            printf 'error: command -v node did not return an absolute path: %s\n' "$node_path" >&2
            exit 1
            ;;
    esac
    node_version=$("$node_path" --version 2>/dev/null || :)
    node_major=${node_version#v}
    node_major=${node_major%%.*}
    case "$node_major" in
        ''|*[!0-9]*)
            printf 'error: could not determine Node.js version from: %s\n' "$node_version" >&2
            exit 1
            ;;
    esac
    if [ "$node_major" -lt 20 ]; then
        printf 'error: Node.js 20 or newer is required; found %s at %s\n' \
            "$node_version" "$node_path" >&2
        exit 1
    fi
    # PATH may select our managed link; resolve it before replacing that link.
    node_path=$("$node_path" -e 'process.stdout.write(require("node:fs").realpathSync(process.argv[1]))' "$node_path")
    if [ -e "$node_link" ] && [ ! -L "$node_link" ]; then
        printf 'error: refusing to replace non-symlink Node path: %s\n' "$node_link" >&2
        exit 1
    fi
    # Earlier installers managed this same Node link. Keep their markers for
    # rollback, but never bypass a present marker with an older one.
    installed_node_marker=$node_marker
    if [ ! -e "$node_marker" ] && [ ! -L "$node_marker" ]; then
        installed_node_marker=$install_root/.aow-runtime-daemon-node-target
        if [ ! -e "$installed_node_marker" ] && [ ! -L "$installed_node_marker" ]; then
            installed_node_marker=$install_root/.aow-terminald-node-target
        fi
    fi
    if [ -L "$node_link" ]; then
        if [ ! -f "$installed_node_marker" ]; then
            printf 'error: refusing to replace unowned Node symlink: %s\n' "$node_link" >&2
            exit 1
        fi
        installed_target=$(cat "$installed_node_marker")
        current_target=$(readlink "$node_link")
        if [ "$installed_target" != "$current_target" ]; then
            printf 'error: Node symlink target does not match installer marker: %s\n' \
                "$node_link" >&2
            exit 1
        fi
    elif [ -e "$installed_node_marker" ]; then
        printf 'error: refusing to reuse orphaned Node marker: %s\n' "$installed_node_marker" >&2
        exit 1
    fi

    mkdir -p "$install_root"
    node_stage=$(mktemp -d "$install_root/.aow-host-daemon-node.start.XXXXXX")
    ln -s "$node_path" "$node_stage/node"
    printf '%s\n' "$node_path" >"$node_stage/.aow-host-daemon-node-target"
    chmod 0600 "$node_stage/.aow-host-daemon-node-target"
    mv -f "$node_stage/node" "$node_link"
    mv -f "$node_stage/.aow-host-daemon-node-target" "$node_marker"
    rmdir "$node_stage"
    node_stage=
}

if ! user_manager_available; then
    printf '%s\n' 'error: a systemd user manager is required to start aow-host-daemon' >&2
    exit 1
fi
if [ ! -x "$launcher" ]; then
    printf '%s\n' 'error: AoW launcher is unavailable; install AoW first' >&2
    exit 1
fi

release=$(resolve_release "$requested_release")
was_active=0
if systemctl --user is-active --quiet "$unit_name"; then
    was_active=1
fi
previous_active=
if [ -L "$active_dir/host-daemon" ]; then
    previous_active=$(readlink "$active_dir/host-daemon")
fi

install_node_link
set_active_release "$release"
if ! install_unit || ! systemctl --user daemon-reload \
    || ! systemctl --user enable --now "$unit_name"; then
    printf '%s\n' 'error: aow-host-daemon service activation failed; restoring the previous active release' >&2
    restore_active_release
    exit 1
fi
if [ "$was_active" -eq 1 ]; then
    if ! systemctl --user restart "$unit_name"; then
        printf '%s\n' 'error: aow-host-daemon restart failed; restoring the previous active release' >&2
        restore_active_release
        if [ -n "$previous_active" ]; then
            systemctl --user restart "$unit_name" || :
        fi
        exit 1
    fi
fi

printf 'aow-host-daemon is active on release %s\n' "$(basename "$release")"
