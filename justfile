set shell := ["bash", "-cu"]

[positional-arguments]
package *args:
    ./scripts/package-release.sh "$@"

[positional-arguments]
install package="target/packages/latest":
    ./scripts/install-release.sh --package "$1"

build-vt-worker:
    cd vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd vt-worker && npm run build

build-frontend:
    cd frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd frontend && npm run build

test-vt-worker:
    cd vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd vt-worker && npm test

build: build-frontend test-vt-worker
    cargo build --workspace

test: test-vt-worker
    if [ "$(uname -s)" = Darwin ]; then TMPDIR="$(node -p 'require("node:fs").realpathSync(require("node:os").tmpdir())')" cargo test --workspace -- --test-threads=4; else cargo test --workspace; fi
    node --test scripts/tests/*.test.mjs
    cd frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd frontend && npm run typecheck

clean:
    cargo clean
    rm -rf frontend/node_modules frontend/dist frontend/*.tsbuildinfo
    rm -rf vt-worker/node_modules

run: build-frontend
    cargo run -p aow-server

run-dev:
    cargo run -p aow-server &
    cd frontend && npm run dev

run-terminald: build-vt-worker
    vt_node=$(command -v node) || { printf '%s\n' 'error: Node.js 20 or newer is required; install Node.js 20+ first' >&2; exit 1; }; if [ -n "${XDG_RUNTIME_DIR:-}" ]; then terminal_runtime_dir="$XDG_RUNTIME_DIR/aow-terminald"; else terminal_runtime_dir="/tmp/aow-terminald-$(id -u)"; fi; install -d -m 700 "$terminal_runtime_dir"; cargo run -p aow-terminald -- --socket "$terminal_runtime_dir/terminald.sock" --vt-node "$vt_node"
