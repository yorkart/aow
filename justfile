set shell := ["bash", "-cu"]

[positional-arguments]
package *args:
    ./scripts/package-release.sh "$@"

[positional-arguments]
install *args:
    bash ./scripts/install-local.sh "$@"

build-vt-worker:
    cd runtime/vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd runtime/vt-worker && npm run build

build-frontend:
    cd frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd frontend && npm run build

test-vt-worker:
    cd runtime/vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd runtime/vt-worker && npm test

build: build-frontend test-vt-worker
    cargo build --workspace
    cargo build --manifest-path runtime/Cargo.toml --workspace --target-dir target

test: test-vt-worker
    if [ "$(uname -s)" = Darwin ]; then TMPDIR="$(node -p 'require("node:fs").realpathSync(require("node:os").tmpdir())')" cargo test --workspace -- --test-threads=4; else cargo test --workspace; fi
    cargo test --manifest-path runtime/Cargo.toml --workspace --target-dir target
    node --test git-platform-providers/tests/*.test.mjs
    node --test scripts/tests/*.test.mjs
    cd frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd frontend && npm run typecheck

clean:
    cargo clean
    cargo clean --manifest-path runtime/Cargo.toml
    rm -rf frontend/node_modules frontend/dist frontend/*.tsbuildinfo
    rm -rf runtime/vt-worker/node_modules

run: build-frontend
    cargo run -p aow-server

run-dev:
    cargo run -p aow-server &
    cd frontend && npm run dev

run-runtime-daemon: build-vt-worker
    cargo run --manifest-path runtime/Cargo.toml -p aow-runtime-daemon

build-runtime: build-vt-worker
    cargo build --manifest-path runtime/Cargo.toml --locked

test-runtime: build-vt-worker
    cargo test --manifest-path runtime/Cargo.toml --locked
