#!/bin/sh
# Called by start-server/start-terminald; terminald's caller has already obtained y.
set -eu
umask 077

component=${1:?service component is required}
requested=${2:-latest}
case "$component" in server|terminald) ;; *) exit 1 ;; esac
script_dir=$(CDPATH= cd -P "$(dirname "$0")" && pwd)
repo_root=$(CDPATH= cd -P "$script_dir/.." && pwd)
runtime_root=${AOW_RUNTIME_ROOT:-${HOME:?HOME must be set}/.local/lib/aow}
label=org.aow.$component
domain=gui/$(id -u)
service=$domain/$label
plist_dir=$HOME/Library/LaunchAgents
plist=$plist_dir/$label.plist
active=$runtime_root/active/$component
stage=
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }
rename() { node -e 'require("node:fs").renameSync(process.argv[1], process.argv[2])' "$1" "$2"; }
cleanup() { [ -z "$stage" ] || rm -rf -- "$stage"; }
trap cleanup 0

command -v launchctl >/dev/null && launchctl print "$domain" >/dev/null 2>&1 \
    || fail 'a logged-in macOS graphical user session is required (launchd gui domain)'
case "$requested" in
    latest) candidate=$runtime_root/latest ;;
    ''|[!A-Za-z0-9]*|*[!A-Za-z0-9._-]*) fail "invalid release id: $requested" ;;
    *) candidate=$runtime_root/releases/$requested ;;
esac
[ -d "$candidate" ] || fail "AOW release is unavailable: $candidate"
release=$(CDPATH= cd -P "$candidate" && pwd)
releases=$(CDPATH= cd -P "$runtime_root/releases" && pwd)
case "$release" in "$releases"/*) ;; *) fail 'release resolves outside the releases directory' ;; esac
[ -x "$release/bin/aow-$component" ] && [ -x "$runtime_root/bin/aow-$component" ] \
    || fail 'AOW binary or launcher is unavailable; run the installer first'
if [ "$component" = server ]; then
    [ -f "$release/frontend/dist/index.html" ] || fail 'incomplete server frontend'
    bash "$repo_root/packaging/bin/aow" pin --if-missing
fi
[ ! -e "$active" ] || [ -L "$active" ] || fail 'active release must be a symlink'
[ ! -L "$plist" ] && [ ! -d "$plist" ] || fail 'LaunchAgent destination must be a regular file'
mkdir -p "$plist_dir" "$runtime_root/active"
stage=$(mktemp -d "$runtime_root/.launchd-$component.XXXXXX")
node_path=$(command -v node)
case "$node_path" in /*) ;; *) fail 'Node.js must resolve to an absolute path' ;; esac
[ -f "$script_dir/service-health.mjs" ] || fail 'service health helper is missing; reinstall the package'
node "$script_dir/launchd-service.mjs" "$component" "$runtime_root" "$stage/service.plist" "$node_path"
plutil -lint "$stage/service.plist" >/dev/null
previous_active=
if [ -L "$active" ]; then previous_active=$(readlink "$active"); fi
if [ -f "$plist" ]; then cp "$plist" "$stage/previous.plist"; fi
previous_node=
if [ "$component" = terminald ] && [ -L "$runtime_root/node" ]; then
    previous_node=$(readlink "$runtime_root/node")
    cp "$runtime_root/.aow-terminald-node-target" "$stage/previous-node-marker"
fi
was_loaded=0
if launchctl print "$service" >/dev/null 2>&1; then was_loaded=1; fi
if [ "$was_loaded" = 1 ] && [ ! -f "$stage/previous.plist" ]; then
    fail 'refusing to replace a loaded LaunchAgent without its installed plist'
fi
changed=0
node_changed=0
unload_job() {
    if launchctl print "$service" >/dev/null 2>&1; then
        # bootout only requests removal; the job can remain registered while
        # its process exits. A repeated request may fail during that removal.
        launchctl bootout "$service" || :
    fi
    unload_attempt=0
    while launchctl print "$service" >/dev/null 2>&1; do
        if [ "$unload_attempt" -ge 300 ]; then
            printf 'error: timed out waiting for launchd to remove %s\n' "$service" >&2
            return 1
        fi
        unload_attempt=$((unload_attempt + 1))
        sleep 0.1
    done
}
rollback() {
    [ "$changed" = 1 ] || return 0
    printf 'Restoring previous %s release and LaunchAgent.\n' "$component" >&2
    unload_job || return 1
    if [ -n "$previous_active" ]; then
        ln -s "$previous_active" "$stage/restore-active" || return 1
        rename "$stage/restore-active" "$active" || return 1
    else
        rm -f "$active" || return 1
    fi
    if [ "$node_changed" = 1 ]; then
        if [ -n "$previous_node" ]; then
            ln -s "$previous_node" "$stage/restore-node" || return 1
            rename "$stage/restore-node" "$runtime_root/node" || return 1
            rename "$stage/previous-node-marker" "$runtime_root/.aow-terminald-node-target" || return 1
        else
            rm -f "$runtime_root/node" "$runtime_root/.aow-terminald-node-target" || return 1
        fi
    fi
    if [ -f "$stage/previous.plist" ]; then
        rename "$stage/previous.plist" "$plist" || return 1
        if [ "$was_loaded" = 1 ]; then
            launchctl bootstrap "$domain" "$plist" || return 1
        fi
    else
        rm -f "$plist" || return 1
    fi
    changed=0
}
trap 'rollback || printf "error: failed to restore previous %s LaunchAgent\n" "$component" >&2; exit 1' 1 2 15
activate() {
    # Prepare everything before stopping the old job. bootout must finish before
    # switching the launcher target, otherwise KeepAlive can restart the old job.
    changed=1
    if [ "$was_loaded" = 1 ]; then unload_job || return; fi
    ln -s "$release" "$stage/active" || return
    rename "$stage/active" "$active" || return
    if [ "$component" = terminald ]; then
        node_changed=1
        rename "$stage/node" "$runtime_root/node" || return
        rename "$stage/.aow-terminald-node-target" "$runtime_root/.aow-terminald-node-target" || return
    fi
    rename "$stage/service.plist" "$plist" || return
    launchctl enable "$service" || return
    launchctl bootstrap "$domain" "$plist" || return
    # RunAtLoad starts the job. Do not kickstart -k here: a second terminald
    # restart could destroy shells opened during activation.
    node "$script_dir/service-health.mjs" "$stage/health.json" "$service"
}
if ! activate; then
    if ! rollback; then
        printf 'error: failed to restore previous %s LaunchAgent\n' "$component" >&2
    fi
    fail "aow-$component activation failed; inspect launchctl print $service and Unified Logging (subsystem org.aow, category $component)"
fi
changed=0
printf 'aow-%s is active on release %s\n' "$component" "$(basename "$release")"
