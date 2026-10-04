#!/usr/bin/env bash
set -euo pipefail

script_dir=$(CDPATH='' cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
package=target/packages/latest
target_user=
package_set=0
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }
while (( $# )); do
    case "$1" in
        --user|--package)
            (( $# >= 2 )) && [[ -n "$2" ]] || fail "$1 requires a value"
            if [[ "$1" == --user ]]; then target_user=$2
            else package=$2; package_set=1; fi
            shift 2 ;;
        -h|--help)
            printf '%s\n' 'Usage: just install [PACKAGE] [--user USER]' \
                'PACKAGE defaults to target/packages/latest; --package PACKAGE is also accepted.' \
                'Without --user, install for the current account.' \
                'On macOS, administrators can specify another account and complete service registration.'
            exit 0 ;;
        -*) fail "unknown argument: $1" ;;
        *)
            [[ "$package_set" == 0 ]] || fail 'only one package may be specified'
            package=$1; package_set=1; shift ;;
    esac
done
if [[ $(uname -s) == Darwin ]]; then
    args=(--package "$package")
    if [[ -n "$target_user" ]]; then args+=(--user "$target_user"); fi
    exec /usr/bin/python3 -I "$script_dir/install-macos.py" "${args[@]}"
fi
[[ -z "$target_user" ]] || fail '--user is only supported by the macOS installer'
exec bash "$script_dir/install-release.sh" --package "$package"
