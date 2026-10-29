#!/usr/bin/env bash
set -euo pipefail
# shellcheck source=scripts/orbit/common.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/common.sh"
orbit_require_node
orbit_repo_dir="$(orbit_repository_root)"

if [[ ! -f "$orbit_repo_dir/server-orbit/dist/main.js" || ! -f "${ORBIT_FRONTEND_DIR:-$orbit_repo_dir/frontend-orbit/dist}/index.html" ]]; then
  bash "$orbit_repo_dir/scripts/orbit/build.sh"
fi

# First launch asks for a username and a hidden, confirmed password.
# Existing accounts are preserved; use account.sh to explicitly update them.
node "$orbit_repo_dir/server-orbit/dist/cli.js" setup
exec node "$orbit_repo_dir/server-orbit/dist/main.js"
