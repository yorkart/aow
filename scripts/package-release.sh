#!/usr/bin/env bash

set -euo pipefail
umask 077

script_dir=$(CDPATH='' cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(CDPATH='' cd -P -- "$script_dir/.." && pwd)
stage=
lock_dir=$repo_root/target/.package.lock
lock_acquired=0
cleanup() {
    [[ -z "$stage" ]] || rm -rf -- "$stage"
    if [[ "$lock_acquired" == 1 ]]; then rmdir -- "$lock_dir" || :; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
    cat <<'EOF'
Usage: package-release.sh [--version VERSION] [--output-dir DIRECTORY] [--skip-build]

Build native Linux/musl or macOS binaries and frontend, then create an installable package.
Does not install, publish or touch services. Prints the package's absolute path.

  --version VERSION       Default: UTC timestamp + Git SHA.
  --output-dir DIRECTORY  Default: <repository>/target/packages.
  --skip-build            Use target/<native Rust target>/release and frontend/dist.
  -h, --help              Show help.

Creates an adjacent .sha256 file for installation with just install.
The output directory's latest symlink selects the last successfully built package.
Requires x86_64/aarch64 Linux or macOS, Node.js 20+, Git and tar.
Building requires npm and Cargo. Linux also needs musl-gcc, the Rust musl target
and readelf. macOS needs Xcode Command Line Tools (clang, lipo and otool).
EOF
}

main() {
    local version='' output_dir=$repo_root/target/packages skip_build=0
    while (( $# )); do
        case "$1" in
            --version|--output-dir)
                if (( $# < 2 )) || [[ -z "$2" ]]; then fail "$1 requires a value"; fi
                if [[ "$1" == --version ]]; then version=$2; else output_dir=$2; fi
                shift 2 ;;
            --skip-build) skip_build=1; shift ;;
            -h|--help) usage; return ;;
            *) fail "unknown argument: $1" ;;
        esac
    done
    if [[ -n "$version" ]]; then
        [[ "$version" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$version" != latest ]] || fail "invalid version: $version"
    fi
    local os machine target artifact_dir binary_arch
    case "$(uname -s)" in
        Linux) os=linux ;;
        Darwin) os=macos ;;
        *) fail 'release packages support Linux and macOS' ;;
    esac
    machine=$(uname -m)
    case "$machine" in
        x86_64) binary_arch=x86_64 ;;
        aarch64|arm64) machine=aarch64; binary_arch=arm64 ;;
        *) fail "unsupported release architecture: $machine" ;;
    esac
    if [[ "$os" == linux ]]; then target=$machine-unknown-linux-musl
    else target=$machine-apple-darwin; fi
    artifact_dir=$repo_root/target/$target/release
    local command
    for command in node git tar; do command -v "$command" >/dev/null || fail "$command is required"; done
    if [[ "$os" == linux ]]; then
        command -v readelf >/dev/null || fail 'readelf is required'
    else
        for command in lipo otool; do command -v "$command" >/dev/null || fail "$command is required (install Xcode Command Line Tools)"; done
    fi
    node -e 'if (Number(process.versions.node.split(".")[0]) < 20) process.exit(1)' || fail 'Node.js 20+ is required'
    local revision short_revision git_status
    revision=$(git -C "$repo_root" rev-parse HEAD)
    short_revision=$(git -C "$repo_root" rev-parse --short=12 HEAD)
    git_status=$(git -C "$repo_root" status --porcelain=v1)
    mkdir -p -- "$repo_root/target" "$output_dir"
    output_dir=$(CDPATH='' cd -P -- "$output_dir" && pwd)
    mkdir -- "$lock_dir" 2>/dev/null || fail "another package build is running: $lock_dir"
    lock_acquired=1
    if [[ -z "$version" ]]; then
        local base suffix=1
        base=$(date -u +%Y%m%dT%H%M%SZ)-$short_revision
        version=$base
        while [[ -e "$output_dir/aow-$version.tar.gz" || -L "$output_dir/aow-$version.tar.gz" \
            || -e "$output_dir/aow-$version.tar.gz.sha256" || -L "$output_dir/aow-$version.tar.gz.sha256" ]]; do
            version=$base-$suffix
            (( suffix += 1 ))
        done
    fi
    local archive=$output_dir/aow-$version.tar.gz
    [[ ! -e "$archive" && ! -L "$archive" ]] || fail "package version already exists: $version"
    [[ ! -e "$archive.sha256" && ! -L "$archive.sha256" ]] || fail "package checksum already exists: $version"
    [[ ! -e "$output_dir/latest" || -L "$output_dir/latest" ]] || fail 'latest must be a symlink'
    if (( ! skip_build )); then
        command -v npm >/dev/null || fail 'npm is required'
        command -v cargo >/dev/null || fail 'Cargo is required'
        command -v rustc >/dev/null || fail 'rustc is required'
        local target_libdir compiler_variable compiler
        target_libdir=$(rustc --print target-libdir --target "$target")
        compgen -G "$target_libdir/libstd-*.rlib" >/dev/null \
            || fail "Rust target is missing; run: rustup target add $target"
        if [[ "$os" == linux ]]; then
            compiler_variable=CC_${target//-/_}
            compiler=${!compiler_variable:-musl-gcc}
            command -v "$compiler" >/dev/null \
                || fail "musl C compiler is missing: $compiler (Debian/Ubuntu: install musl-tools)"
            export "$compiler_variable=$compiler"
        else
            command -v clang >/dev/null || fail 'clang is required (install Xcode Command Line Tools)'
            export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
        fi
        (
            cd "$repo_root/frontend"
            npm ci --ignore-scripts --no-audit --no-fund
            npm run build
        ) >&2
        (
            cd "$repo_root/vt-worker"
            npm ci --ignore-scripts --no-audit --no-fund
            npm run build
        ) >&2
        (
            cd "$repo_root"
            cargo build --locked --release --target "$target" --target-dir "$repo_root/target" -p aow-server
            cargo build --locked --release --target "$target" --target-dir "$repo_root/target" -p aow-terminald
            cargo build --locked --release --target "$target" --target-dir "$repo_root/target" -p aow-automations --bin aow-automation-runner
            cargo build --locked --release --target "$target" --target-dir "$repo_root/target" -p aow-cli
        ) >&2
    fi
    stage=$(mktemp -d "$output_dir/.package-$version.XXXXXX")
    local bundle=$stage/bundle entry elf_info
    mkdir -p "$bundle/bin" "$bundle/frontend"
    for entry in aow-server aow-terminald aow-automation-runner aow-cli; do
        [[ -f "$artifact_dir/$entry" ]] || fail "missing artifact: $artifact_dir/$entry; build the $target release first"
        install -m 0755 -- "$artifact_dir/$entry" "$bundle/bin/$entry"
        # Verify every staged binary, including when --skip-build reuses files.
        # A musl-named directory alone does not prove the contents are portable.
        if [[ "$os" == linux ]]; then
            elf_info=$(LC_ALL=C readelf -l -d -V "$bundle/bin/$entry") \
                || fail "invalid ELF executable: $entry"
            if [[ "$elf_info" == *INTERP* || "$elf_info" == *'(NEEDED)'* || "$elf_info" == *GLIBC_* ]]; then
                fail "$entry is not a static musl release; rebuild with target $target (dynamic/GLIBC dependencies found)"
            fi
        else
            [[ $(lipo -archs "$bundle/bin/$entry") == "$binary_arch" ]] \
                || fail "$entry must be a native $binary_arch Mach-O executable"
            otool -L "$bundle/bin/$entry" >"$stage/libraries"
            node --input-type=module - "$stage/libraries" <<'JS'
import { readFileSync } from 'node:fs';
for (const line of readFileSync(process.argv[2], 'utf8').trim().split('\n').slice(1)) {
    const library = line.trim().split(' (')[0];
    if (!library.startsWith('/usr/lib/') && !library.startsWith('/System/Library/')) {
        throw new Error(`Release depends on a non-system library: ${library}`);
    }
}
JS
            otool -l "$bundle/bin/$entry" >"$stage/load-commands"
            node --input-type=module - "$stage/load-commands" <<'JS' >>"$stage/macos-minimums"
import { readFileSync } from 'node:fs';
const commands = readFileSync(process.argv[2], 'utf8').split(/Load command \d+/);
const versions = commands.flatMap(command => {
    if (!/cmd LC_(BUILD_VERSION|VERSION_MIN_MACOSX)\b/.test(command)) return [];
    const version = command.match(/^\s*(?:minos|version)\s+(\d+\.\d+(?:\.\d+)?)\s*$/m)?.[1];
    if (!version) throw new Error('Missing Mach-O minimum macOS version');
    return [version];
});
if (!versions.length) throw new Error('Missing Mach-O deployment target');
console.log(versions.join('\n'));
JS
        fi
        "$bundle/bin/$entry" --help >/dev/null
    done
    [[ -f "$repo_root/frontend/dist/index.html" ]] || fail 'missing frontend/dist/index.html'
    cp -R -- "$repo_root/frontend/dist" "$bundle/frontend/dist"
    for entry in scripts/start-server.sh scripts/start-terminald.sh scripts/replace-symlink.mjs scripts/server-state-dir.sh \
        scripts/start-launchd.sh scripts/launchd-service.mjs scripts/activate-terminald.mjs scripts/service-health.mjs \
        packaging/bin/aow packaging/bin/aow-server packaging/bin/aow-terminald \
        packaging/systemd/aow-server.service packaging/systemd/aow-terminald.service; do
        mkdir -p -- "$(dirname "$bundle/$entry")"
        install -m 0644 -- "$repo_root/$entry" "$bundle/$entry"
    done
    chmod 0755 "$bundle/packaging/bin/"* "$bundle/scripts/"*.sh
    printf '%s' "$git_status" >"$stage/git-status"
    node --input-type=module - "$bundle" "$version" "$revision" "$stage/git-status" "$skip_build" "$target" "$machine" "$os" "$stage/macos-minimums" <<'JS'
import { createHash } from 'node:crypto';
import { lstatSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
const [root, version, revision, statusPath, skipBuild, target, arch, os, minimums] = process.argv.slice(2);
const compareVersions = (a, b) => {
    const left = a.split('.').map(Number), right = b.split('.').map(Number);
    for (let i = 0; i < 3; i++) { const difference = (left[i] || 0) - (right[i] || 0); if (difference) return difference; }
    return 0;
};
const minimumOsVersion = os === 'macos'
    ? readFileSync(minimums, 'utf8').trim().split('\n').sort(compareVersions).at(-1) : undefined;
function validate(directory) {
    for (const name of readdirSync(directory)) {
        if (/[\\\r\n]/.test(name)) throw new Error(`Unsupported archive filename: ${name}`);
        const path = join(directory, name), info = lstatSync(path);
        if (info.isDirectory()) validate(path);
        else if (!info.isFile() || info.nlink !== 1) throw new Error(`Only unlinked regular files can be packaged: ${path}`);
    }
}
validate(root);
const status = readFileSync(statusPath);
writeFileSync(join(root, 'manifest.json'), JSON.stringify({
    release_id: version, created_at: new Date().toISOString(), git_revision: revision,
    git_dirty: status.length > 0, git_status_sha256: createHash('sha256').update(status).digest('hex'),
    reused_build_artifacts: skipBuild === '1',
    platform: os === 'linux' ? { os, arch, target, libc: 'musl', linkage: 'static' }
        : { os, arch, target, linkage: 'dynamic', minimum_os_version: minimumOsVersion },
    components: { server: 'bin/aow-server', terminald: 'bin/aow-terminald',
        runner: 'bin/aow-automation-runner', cli: 'bin/aow-cli', frontend: 'frontend/dist' },
}, null, 2) + '\n');
JS
    COPYFILE_DISABLE=1 tar -czf "$stage/archive.tar.gz" -C "$bundle" .
    tar -tzf "$stage/archive.tar.gz" >/dev/null
    node --input-type=module - "$stage/archive.tar.gz" "$archive" <<'JS'
import { createHash } from 'node:crypto';
import { chmodSync, createReadStream, linkSync, unlinkSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';
const [source, archive] = process.argv.slice(2);
const hash = createHash('sha256');
for await (const chunk of createReadStream(source)) hash.update(chunk);
writeFileSync(`${source}.sha256`, `${hash.digest('hex')}  ${basename(archive)}\n`);
chmodSync(source, 0o644);
chmodSync(`${source}.sha256`, 0o644);
linkSync(source, archive);
try {
    linkSync(`${source}.sha256`, `${archive}.sha256`);
} catch (error) {
    unlinkSync(archive);
    throw error;
}
JS
    ln -s -- "aow-$version.tar.gz" "$stage/latest"
    node -e 'require("node:fs").renameSync(process.argv[1], process.argv[2])' "$stage/latest" "$output_dir/latest"
    printf '%s\n' "$archive"
}
main "$@"
