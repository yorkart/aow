#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=scripts/orbit/common.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
orbit_require_node
orbit_repo_dir="$(orbit_repository_root)"

npm --prefix "$orbit_repo_dir/server-orbit" ci --ignore-scripts --no-audit --no-fund
npm --prefix "$orbit_repo_dir/server-orbit" run build
npm --prefix "$orbit_repo_dir/frontend-orbit" ci --ignore-scripts --no-audit --no-fund
npm --prefix "$orbit_repo_dir/frontend-orbit" run build
