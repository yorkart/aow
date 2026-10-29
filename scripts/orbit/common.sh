#!/usr/bin/env bash

# Sourced only by Orbit scripts; never uses AoW's installation or state paths.
orbit_repository_root() {
  (cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
}

orbit_require_node() {
  if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
    printf '%s\n' 'Orbit requires Node.js 22.16+ (22.x) or Node.js 24+ and npm.' >&2
    exit 1
  fi
  if ! node -e 'const [major, minor] = process.versions.node.split(".").map(Number); process.exit((major === 22 && minor >= 16) || major >= 24 ? 0 : 1)'; then
    printf '%s\n' 'Orbit requires Node.js 22.16+ (22.x) or Node.js 24+.' >&2
    exit 1
  fi
}
