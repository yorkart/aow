#!/bin/sh

set -eu

# Require confirmation before changing runtime files or touching the service.
printf '%s\n' '警告：此操作将启动或重启 terminald，重启会终止它管理的所有终端会话。' >&2
printf '%s' '请输入 y 并回车以继续，其他输入取消: ' >&2
if ! IFS= read -r confirmation || [ "$confirmation" != y ]; then
    printf '%s\n' '已取消，terminald 保持当前状态。' >&2
    exit 1
fi

script_dir=$(CDPATH='' cd -P "$(dirname "$0")" && pwd)
repo_root=$(CDPATH='' cd -P "$script_dir/.." && pwd)
runtime_root=${AOW_RUNTIME_ROOT:-${HOME:?HOME must be set}/.local/lib/aow}
releases_dir=$runtime_root/releases
active_dir=$runtime_root/active
runtime_bin_dir=$runtime_root/bin
unit_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
unit_name=aow-terminald.service
unit_source=$repo_root/packaging/systemd/aow-terminald.service
node_link=$runtime_root/node
node_marker=$runtime_root/.aow-terminald-node-target
requested_release=${1:-latest}
if [ "$(uname -s)" = Darwin ]; then
    exec node "$script_dir/activate-terminald.mjs" "$script_dir/start-launchd.sh" terminald "$requested_release"
fi
node_stage=

cleanup() {
    if [ -n "$node_stage" ]; then
        rm -f "$node_stage/node" "$node_stage/.aow-terminald-node-target"
        rmdir "$node_stage" 2>/dev/null || :
    fi
}
trap cleanup 0
trap 'exit 1' 1 2 15

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
        printf 'error: AOW release is unavailable: %s\n' "$candidate" >&2
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
    if [ ! -x "$resolved/bin/aow-terminald" ]; then
        printf 'error: incomplete AOW terminald release: %s\n' "$resolved" >&2
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
    active_path=$active_dir/terminald
    if [ -e "$active_path" ] && [ ! -L "$active_path" ]; then
        printf 'error: active terminald path is not a symlink: %s\n' "$active_path" >&2
        exit 1
    fi
    active_tmp=$active_dir/.terminald.$$.tmp
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
        restore_tmp=$active_dir/.terminald.restore.$$.tmp
        rm -f "$restore_tmp"
        ln -s "$previous_active" "$restore_tmp"
        replace_symlink "$restore_tmp" "$active_dir/terminald"
    else
        rm -f "$active_dir/terminald"
    fi
}

install_node_link() {
    if ! command -v node >/dev/null 2>&1; then
        printf '%s\n' 'error: Node.js 20 or newer is required by aow-terminald' >&2
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
    if [ -e "$node_link" ] && [ ! -L "$node_link" ]; then
        printf 'error: refusing to replace non-symlink Node path: %s\n' "$node_link" >&2
        exit 1
    fi
    if [ -L "$node_link" ]; then
        if [ ! -f "$node_marker" ]; then
            printf 'error: refusing to replace unowned Node symlink: %s\n' "$node_link" >&2
            exit 1
        fi
        installed_target=$(cat "$node_marker")
        current_target=$(readlink "$node_link")
        if [ "$installed_target" != "$current_target" ]; then
            printf 'error: Node symlink target does not match installer marker: %s\n' \
                "$node_link" >&2
            exit 1
        fi
    elif [ -e "$node_marker" ]; then
        printf 'error: refusing to reuse orphaned Node marker: %s\n' "$node_marker" >&2
        exit 1
    fi

    mkdir -p "$runtime_root"
    node_stage=$(mktemp -d "$runtime_root/.aow-terminald-node.start.XXXXXX")
    ln -s "$node_path" "$node_stage/node"
    printf '%s\n' "$node_path" >"$node_stage/.aow-terminald-node-target"
    chmod 0600 "$node_stage/.aow-terminald-node-target"
    mv -f "$node_stage/node" "$node_link"
    mv -f "$node_stage/.aow-terminald-node-target" "$node_marker"
    rmdir "$node_stage"
    node_stage=
}

if ! user_manager_available; then
    printf '%s\n' 'error: a systemd user manager is required to start aow-terminald' >&2
    exit 1
fi
if [ ! -x "$runtime_bin_dir/aow-terminald" ]; then
    printf '%s\n' 'error: AOW launcher is unavailable; install AOW first' >&2
    exit 1
fi

release=$(resolve_release "$requested_release")
was_active=0
if systemctl --user is-active --quiet "$unit_name"; then
    was_active=1
fi
previous_active=
if [ -L "$active_dir/terminald" ]; then
    previous_active=$(readlink "$active_dir/terminald")
fi

# This is intentionally the only path that changes the terminald service or
# its managed Node link. The installer only activates it after confirmation.
install_node_link
set_active_release "$release"
if ! install_unit || ! systemctl --user daemon-reload \
    || ! systemctl --user enable --now "$unit_name"; then
    printf '%s\n' 'error: aow-terminald service activation failed; restoring the previous active release' >&2
    restore_active_release
    exit 1
fi
if [ "$was_active" -eq 1 ]; then
    if ! systemctl --user restart "$unit_name"; then
        printf '%s\n' 'error: aow-terminald restart failed; restoring the previous active release' >&2
        restore_active_release
        if [ -n "$previous_active" ]; then
            systemctl --user restart "$unit_name" || :
        fi
        exit 1
    fi
fi

printf 'aow-terminald is active on release %s\n' "$(basename "$release")"
