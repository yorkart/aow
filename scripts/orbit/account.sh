#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=scripts/orbit/common.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
orbit_require_node
orbit_repo_dir="$(orbit_repository_root)"

if [[ ! -f "$orbit_repo_dir/server-orbit/dist/cli.js" ]]; then
  npm --prefix "$orbit_repo_dir/server-orbit" ci --ignore-scripts --no-audit --no-fund
  npm --prefix "$orbit_repo_dir/server-orbit" run build
fi
exec node "$orbit_repo_dir/server-orbit/dist/cli.js" account
