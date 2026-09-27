#!/usr/bin/env bash

set -euo pipefail
umask 077

# GitHub publication sets the repository in the distributed copy (including forks).
DEFAULT_REPOSITORY=yorkart/aow
# GitHub publication pins this copy to its release, even if latest moves mid-install.
DEFAULT_GITHUB_VERSION=''
stage=
lock_dir=
lock_acquired=0
pending_file=

cleanup() {
    [[ -z "$pending_file" ]] || rm -f -- "$pending_file"
    [[ -z "$stage" ]] || rm -rf -- "$stage"
    if [[ "$lock_acquired" == 1 ]]; then
        rmdir -- "$lock_dir" || :
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
    cat <<EOF
Usage: install-release.sh [--repo OWNER/REPO] [--version VERSION]
       install-release.sh --package FILE [--repo OWNER/REPO]

Install a GitHub Release or local package, then start/restart its user services.
Without --version, install this script's release (or resolve latest from source).
With --version, download that release directly. All packages require SHA256 verification.
Only a lowercase y entered at the terminal permits starting/restarting terminald.
A missing pin.md5 must be created interactively before starting the server.

Options:
  --repo OWNER/REPO GitHub repository (default: $DEFAULT_REPOSITORY).
                    Also configurable with AOW_RELEASE_REPOSITORY.
  --version VERSION Install a specific GitHub release tag.
  --package FILE   Install a local archive with its adjacent FILE.sha256.
                    Resolves symlinks once; reads the version from the manifest.
                    Cannot be combined with --version. No downloads required.
  -h, --help        Show this help.

Requires Bash, tar, Node.js 20+, Git, and Linux/systemd or macOS/launchd.
macOS installation requires a logged-in graphical user session.
Downloads require curl and HTTPS access to GitHub Releases.
Installs under ~/.local/lib/aow; preserves existing configuration and data.
Also installs aow update / aow pin and remembers the GitHub repository.
Local installs preserve the saved repository unless explicitly overridden.
EOF
}

valid_version() {
    [[ "$1" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ ]] && [[ "$1" != latest ]]
}

download() {
    printf 'Downloading %s\n' "$1"
    curl --fail --show-error --location --retry 3 --connect-timeout 15 \
        --proto '=https' --proto-redir '=https' --output "$2" "$1"
}

download_github_release() {
    local release_root=$1 asset=aow-$os-$arch.tar.gz resolved_url tag_prefix
    if (( ! explicit_version )); then
        if [[ "$repository" == "$DEFAULT_REPOSITORY" && -n "$DEFAULT_GITHUB_VERSION" ]]; then
            version=$DEFAULT_GITHUB_VERSION
        else
            # The source installer and source overrides resolve latest once.
            # All payload and checksum requests then use the immutable tag URL.
            resolved_url=$(curl --fail --show-error --location --head --retry 3 --connect-timeout 15 \
                --proto '=https' --proto-redir '=https' --output /dev/null \
                --write-out '%{url_effective}' "$release_root/latest")
            tag_prefix=$release_root/tag/
            [[ "$resolved_url" == "$tag_prefix"* ]] || fail 'could not resolve the latest GitHub release'
            version=${resolved_url#"$tag_prefix"}
        fi
    fi
    valid_version "$version" || fail 'invalid GitHub release version'
    local download_dir=$release_root/download/$version
    download "$download_dir/SHA256SUMS" "$stage/SHA256SUMS"
    download "$download_dir/$asset" "$stage/release.tar.gz"
}

stage_local_package() {
    node --input-type=module - "$1" "$stage" <<'JS'
import { copyFileSync, realpathSync, statSync } from 'node:fs';
import { basename, join } from 'node:path';
const [source, stage] = process.argv.slice(2);
try {
    // Freeze latest before selecting its checksum or copying either file.
    const archive = realpathSync(source);
    const checksum = `${archive}.sha256`;
    if (!statSync(archive).isFile() || !statSync(checksum).isFile()) {
        throw new Error('expected a package and its adjacent .sha256 file');
    }
    copyFileSync(checksum, join(stage, 'SHA256SUMS'));
    copyFileSync(archive, join(stage, 'release.tar.gz'));
    process.stdout.write(basename(archive));
} catch (error) {
    console.error(`error: cannot load local package ${source}: ${error.message}. Build it with just package first.`);
    process.exit(1);
}
JS
}

verify_checksum() {
    node --input-type=module - "$stage/SHA256SUMS" "$stage/release.tar.gz" "$1" <<'JS'
import { createHash } from 'node:crypto';
import { createReadStream, readFileSync } from 'node:fs';
const [checksums, archive, name] = process.argv.slice(2);
const matches = readFileSync(checksums, 'utf8').trim().split(/\r?\n/)
    .map(line => line.match(/^([a-fA-F0-9]{64}) [ *](\S+)$/))
    .filter(match => match?.[2] === name);
if (matches.length !== 1) throw new Error(`Expected one SHA256 checksum for ${name}`);
const hash = createHash('sha256');
for await (const chunk of createReadStream(archive)) hash.update(chunk);
if (hash.digest('hex') !== matches[0][1].toLowerCase()) throw new Error(`SHA256 checksum mismatch for ${name}`);
JS
}

validate_release() {
    node --input-type=module - "$1" "$2" "$os" "$arch" <<'JS'
import { lstatSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
const [root, version, os, arch] = process.argv.slice(2);
try {
    const manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
    const id = manifest.release_id;
    if (typeof id !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(id)
        || /[\r\n]/.test(id) || id === 'latest') throw new Error('invalid manifest.release_id');
    if (version && id !== version) throw new Error('manifest.release_id does not match requested version');
    if (manifest.platform?.os !== os || manifest.platform?.arch !== arch) {
        throw new Error(`package platform does not match ${os}-${arch}`);
    }
    if (os === 'macos' && manifest.platform.minimum_os_version != null) {
        const minimum = manifest.platform.minimum_os_version;
        const current = execFileSync('sw_vers', ['-productVersion'], { encoding: 'utf8' }).trim();
        if (![minimum, current].every(value => typeof value === 'string' && /^\d+\.\d+(?:\.\d+)?$/.test(value))) {
            throw new Error('invalid macOS version');
        }
        const left = current.split('.').map(Number), right = minimum.split('.').map(Number);
        for (let i = 0; i < 3; i++) {
            if ((left[i] || 0) > (right[i] || 0)) break;
            if ((left[i] || 0) < (right[i] || 0)) throw new Error(`package requires macOS ${minimum} or newer (current: ${current})`);
        }
    }
    for (const path of ['manifest.json', 'bin', 'frontend', 'frontend/dist',
        'bin/aow-server', 'bin/aow-terminald',
        'bin/aow-automation-runner', 'bin/aow-cli', 'frontend/dist/index.html']) {
        const info = lstatSync(join(root, path));
        const directory = ['bin', 'frontend', 'frontend/dist'].includes(path);
        if (!(directory ? info.isDirectory() : info.isFile())) throw new Error(`Invalid release entry: ${path}`);
    }
    process.stdout.write(id);
} catch (error) {
    console.error(`error: invalid AoW release: ${error.message}`);
    process.exit(1);
}
JS
}

compare_release() {
    node --input-type=module - "$1" "$2" <<'JS'
import { lstatSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
const [installed, incoming] = process.argv.slice(2);
function equal(path) {
    const left = join(installed, path), right = join(incoming, path);
    const a = lstatSync(left), b = lstatSync(right);
    if (a.isFile() && b.isFile()) return readFileSync(left).equals(readFileSync(right));
    if (!a.isDirectory() || !b.isDirectory()) return false;
    const names = readdirSync(left).sort();
    return JSON.stringify(names) === JSON.stringify(readdirSync(right).sort())
        && names.every(name => equal(join(path, name)));
}
try {
    const paths = ['manifest.json', 'bin', 'frontend', 'scripts', 'packaging'];
    if (!paths.every(equal)) throw new Error('content differs');
} catch (error) {
    console.error(`error: version already installed with different content; use a new version number (${error.message})`);
    process.exit(1);
}
JS
}

replace_file() {
    pending_file=$(mktemp "$(dirname "$2")/.install.XXXXXX")
    cp -- "$1" "$pending_file"
    chmod "$3" "$pending_file"
    node -e 'require("node:fs").renameSync(process.argv[1], process.argv[2])' "$pending_file" "$2"
    pending_file=
}

replace_link() {
    # Resolve parent symlinks so the helper also runs when HOME is a symlink.
    local helper_dir
    helper_dir=$(CDPATH='' cd -P -- "$support/scripts" && pwd)
    pending_file=$(mktemp "$(dirname "$2")/.link.XXXXXX")
    rm -- "$pending_file"
    ln -s -- "$1" "$pending_file"
    node "$helper_dir/replace-symlink.mjs" "$pending_file" "$2"
    pending_file=
}

terminald_confirmation() {
    # The start script prints the prompt. Its stdin receives only the terminal's
    # reply, never bytes from `curl ... | bash`. Record it to distinguish a skip
    # (the start script exits 1) from a real failure after the user confirmed.
    local answer=
    if { IFS= read -r answer </dev/tty; } 2>/dev/null; then
        printf '%s' "$answer" >"$stage/confirmation"
    else
        answer=
    fi
    printf '%s\n' "$answer"
}

main() {
    local repository=${AOW_RELEASE_REPOSITORY:-}
    local version='' explicit_version=0 package=''
    while (( $# )); do
        case "$1" in
            --repo|--version|--package)
                if (( $# < 2 )) || [[ -z "$2" ]]; then fail "$1 requires a value"; fi
                if [[ "$1" == --repo ]]; then repository=$2
                elif [[ "$1" == --version ]]; then version=$2; explicit_version=1
                else package=$2; fi
                shift 2 ;;
            -h|--help) usage; return ;;
            *) fail "unknown argument: $1" ;;
        esac
    done
    if [[ -n "$package" ]] && (( explicit_version )); then
        fail '--package cannot be combined with --version; local versions come from the manifest'
    fi
    local runtime_root=${HOME:?HOME must be set}/.local/lib/aow
    if [[ -n "$package" && -z "$repository" && -f "$runtime_root/update.json" ]]; then
        repository=$(node --input-type=module - "$runtime_root/update.json" <<'JS'
import { readFileSync } from 'node:fs';
const repository = JSON.parse(readFileSync(process.argv[2], 'utf8')).repository;
if (repository != null && typeof repository !== 'string') throw new Error('invalid saved repository');
process.stdout.write(repository || '');
JS
)
    fi
    repository=${repository:-$DEFAULT_REPOSITORY}
    [[ "$repository" =~ ^[A-Za-z0-9_-]+/[A-Za-z0-9_.-]+$ ]] \
        || fail '--repo must be a GitHub repository in OWNER/REPO form'
    if (( explicit_version )); then valid_version "$version" || fail "invalid version: $version"; fi
    if [[ -z "$package" ]]; then command -v curl >/dev/null || fail 'curl is required'; fi
    local os arch
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) fail 'this installer supports Linux and macOS' ;;
    esac
    case "$(uname -m)" in
        x86_64) arch=x86_64 ;;
        aarch64|arm64) arch=aarch64 ;;
        *) fail 'unsupported installation architecture' ;;
    esac
    local command
    for command in tar node git; do
        command -v "$command" >/dev/null || fail "$command is required"
    done
    node -e 'if (Number(process.versions.node.split(".")[0]) < 20) process.exit(1)' \
        || fail 'Node.js 20 or newer is required'
    if [[ "$os" == linux ]]; then
        if ! command -v systemctl >/dev/null || ! systemctl --user show-environment >/dev/null 2>&1; then
            fail 'a systemd user manager is required'
        fi
    else
        if ! command -v launchctl >/dev/null || ! launchctl print "gui/$(id -u)" >/dev/null 2>&1; then
            fail 'a logged-in macOS graphical user session is required (launchd gui domain)'
        fi
    fi

    # The shipped systemd units use %h/.local/lib/aow.
    [[ ${AOW_RUNTIME_ROOT:-$runtime_root} == "$runtime_root" ]] \
        || fail 'the installer currently requires ~/.local/lib/aow as AOW_RUNTIME_ROOT'
    local user_bin_dir=${AOW_USER_BIN_DIR:-}
    if [[ -z "$user_bin_dir" && -f "$runtime_root/update.json" ]]; then
        user_bin_dir=$(node --input-type=module - "$runtime_root/update.json" <<'JS'
import { readFileSync } from 'node:fs';
const directory = JSON.parse(readFileSync(process.argv[2], 'utf8')).user_bin_dir;
if (directory != null && typeof directory !== 'string') throw new Error('invalid saved command directory');
process.stdout.write(directory || '');
JS
)
    fi
    user_bin_dir=${user_bin_dir:-$HOME/.local/bin}
    [[ "$runtime_root" == /* && "$user_bin_dir" == /* ]] || fail 'installation paths must be absolute'
    export AOW_RUNTIME_ROOT=$runtime_root
    mkdir -p -- "$runtime_root/releases" "$runtime_root/bin" "$runtime_root/active" "$user_bin_dir"
    lock_dir=$runtime_root/.install.lock
    mkdir -- "$lock_dir" 2>/dev/null || fail "another install is running: $lock_dir"
    lock_acquired=1
    stage=$(mktemp -d "$runtime_root/.download.XXXXXX")
    local asset
    if [[ -n "$package" ]]; then
        asset=$(stage_local_package "$package")
    else
        asset=aow-$os-$arch.tar.gz
        download_github_release "https://github.com/$repository/releases"
    fi
    verify_checksum "$asset"

    # Inspect the entire archive before extraction. Only directories and regular
    # files are accepted: links and special files could escape the staging root.
    tar -tzf "$stage/release.tar.gz" >"$stage/entries"
    local entry
    while IFS= read -r entry; do
        case "$entry" in
            /*|..|../*|*/../*|*/..|*\\*) fail "unsafe archive path: $entry" ;;
        esac
    done <"$stage/entries"
    tar -tvzf "$stage/release.tar.gz" >"$stage/types"
    while IFS= read -r entry; do
        [[ "$entry" == [-d]* ]] || fail 'archive must contain only regular files and directories'
    done <"$stage/types"
    mkdir "$stage/unpacked"
    tar -xzf "$stage/release.tar.gz" --no-same-owner --no-same-permissions -C "$stage/unpacked"
    support=$stage/unpacked
    version=$(validate_release "$support" "$version")
    printf 'Installing AoW %s\n' "$version"
    local -a helpers=(
        scripts/start-server.sh scripts/start-terminald.sh scripts/replace-symlink.mjs scripts/server-state-dir.sh
        packaging/bin/aow packaging/bin/aow-server packaging/bin/aow-terminald
    )
    if [[ "$os" == linux ]]; then
        helpers+=(packaging/systemd/aow-server.service packaging/systemd/aow-terminald.service)
    else
        helpers+=(scripts/start-launchd.sh scripts/launchd-service.mjs scripts/activate-terminald.mjs)
    fi
    for entry in "${helpers[@]}"; do
        [[ -f "$support/$entry" ]] || fail "archive is missing $entry"
    done
    chmod 0755 "$support"/bin/* "$support"/packaging/bin/*

    local release=$runtime_root/releases/$version
    if [[ -e "$release" || -L "$release" ]]; then
        [[ -d "$release" && ! -L "$release" ]] || fail "invalid existing release: $release"
        validate_release "$release" "$version" >/dev/null
        compare_release "$release" "$support"
        printf 'Keeping existing immutable release %s\n' "$release"
    else
        release=$support
    fi
    "$release/bin/aow-cli" --help >/dev/null
    local destination
    for destination in "$runtime_root/latest" "$user_bin_dir/aow" "$user_bin_dir/aow-cli" \
        "$user_bin_dir/aow-automation-runner" "$runtime_root/active/server" "$runtime_root/active/terminald"; do
        [[ ! -e "$destination" || -L "$destination" ]] || fail "refusing to replace unmanaged entry: $destination"
    done
    for destination in "$runtime_root/bin/aow-server" "$runtime_root/bin/aow-terminald"; do
        [[ ! -d "$destination" || -L "$destination" ]] || fail "launcher destination is a directory: $destination"
    done
    [[ ! -d "$runtime_root/update.json" ]] || fail 'update.json must be a file, not a directory'
    if [[ -L "$user_bin_dir/aow" ]]; then
        local command_target command_version
        command_target=$(readlink -- "$user_bin_dir/aow")
        command_version=${command_target#"$runtime_root/releases/"}
        command_version=${command_version%/packaging/bin/aow}
        if ! valid_version "$command_version" \
            || [[ "$command_target" != "$runtime_root/releases/$command_version/packaging/bin/aow" ]]; then
            fail "refusing to replace unmanaged entry: $user_bin_dir/aow"
        fi
    fi
    node --input-type=module - "$stage/update.json" "$repository" "$user_bin_dir" <<'JS'
import { writeFileSync } from 'node:fs';
const [destination, repository, userBinDir] = process.argv.slice(2);
writeFileSync(destination, JSON.stringify({ repository, user_bin_dir: userBinDir }, null, 2) + '\n');
JS

    # shellcheck source-path=SCRIPTDIR
    # shellcheck source=server-state-dir.sh
    source "$support/scripts/server-state-dir.sh"
    local state_dir
    state_dir=$(server_state_dir)
    # Use the verified package before changing any installed command or service.
    bash "$support/packaging/bin/aow" pin --if-missing
    "$release/bin/aow-server" --initialize-state-if-missing --state-dir "$state_dir"
    if [[ "$release" == "$support" ]]; then
        release=$runtime_root/releases/$version
        node -e 'require("node:fs").renameSync(process.argv[1], process.argv[2])' "$support" "$release"
        support=$release
    fi
    replace_file "$support/packaging/bin/aow-server" "$runtime_root/bin/aow-server" 0755
    replace_file "$support/packaging/bin/aow-terminald" "$runtime_root/bin/aow-terminald" 0755
    replace_link "$release/packaging/bin/aow" "$user_bin_dir/aow"
    replace_link "releases/$version" "$runtime_root/latest"
    replace_link "$release/bin/aow-automation-runner" "$user_bin_dir/aow-automation-runner"
    replace_link "$release/bin/aow-cli" "$user_bin_dir/aow-cli"
    printf 'Installed AoW %s in %s\n' "$version" "$release"
    case ":${PATH:-}:" in
        *":$user_bin_dir:"*) ;;
        *) printf 'Add this directory to PATH to use aow and aow-cli: %s\n' "$user_bin_dir" ;;
    esac

    # Complete other activation work first: restarting terminald may terminate
    # the terminal running this installer. Release the install lock beforehand.
    sh "$support/scripts/start-server.sh" "$version"
    # Remember the repository only after the server activates successfully.
    replace_file "$stage/update.json" "$runtime_root/update.json" 0600
    rmdir -- "$lock_dir"
    lock_acquired=0
    : >"$stage/confirmation"
    if terminald_confirmation | sh "$support/scripts/start-terminald.sh" "$version"; then
        printf 'AoW %s is installed and its services are active.\n' "$version"
    elif [[ $(cat "$stage/confirmation") != y ]]; then
        printf 'Skipped terminald; Web server, CLI and Runner are updated to %s.\n' "$version"
    else
        fail 'terminald activation failed; the release is installed and Web server is already active'
    fi
}

# Parse the complete installer before executing it, including when piped to bash.
main "$@"
