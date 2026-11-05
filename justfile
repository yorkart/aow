set shell := ["bash", "-cu"]

[positional-arguments]
package *args:
    ./scripts/package-release.sh "$@"

[positional-arguments]
install *args:
    bash ./scripts/install-local.sh "$@"

build-vt-worker:
    cd host/vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd host/vt-worker && npm run build

build-frontend:
    cd server/frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd server/frontend && npm run build

test-vt-worker:
    cd host/vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd host/vt-worker && npm test

build: build-frontend test-vt-worker
    cargo build --manifest-path server/Cargo.toml --target-dir target --workspace
    cargo build --manifest-path host/Cargo.toml --workspace --target-dir target

build-host-test-daemon: build-vt-worker
    cargo build --manifest-path host/Cargo.toml --example host-test-daemon --target-dir target

test: test-vt-worker build-host-test-daemon
    if [ "$(uname -s)" = Darwin ]; then TMPDIR="$(node -p 'require("node:fs").realpathSync(require("node:os").tmpdir())')" AOW_HOST_TEST_DAEMON="$PWD/target/debug/examples/host-test-daemon" cargo test --manifest-path server/Cargo.toml --target-dir target --workspace -- --test-threads=4; else AOW_HOST_TEST_DAEMON="$PWD/target/debug/examples/host-test-daemon" cargo test --manifest-path server/Cargo.toml --target-dir target --workspace; fi
    cargo test --manifest-path common/Cargo.toml --workspace --target-dir target
    cargo test --manifest-path host/Cargo.toml --workspace --target-dir target
    node --test server/git-platform-providers/tests/*.test.mjs
    node --test scripts/tests/*.test.mjs
    cd server/frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd server/frontend && npm run typecheck

clean:
    cargo clean --manifest-path server/Cargo.toml --target-dir target
    cargo clean --manifest-path host/Cargo.toml
    rm -rf server/frontend/node_modules server/frontend/dist server/frontend/*.tsbuildinfo
    rm -rf host/vt-worker/node_modules

run: build-frontend
    cargo run --manifest-path server/Cargo.toml --target-dir target -p aow-server

run-dev:
    cargo run --manifest-path server/Cargo.toml --target-dir target -p aow-server &
    cd server/frontend && npm run dev

run-host-daemon: build-vt-worker
    cargo run --manifest-path host/Cargo.toml -p aow-host-daemon

build-host: build-vt-worker
    cargo build --manifest-path host/Cargo.toml --locked

test-host: build-vt-worker
    cargo test --manifest-path host/Cargo.toml --locked
