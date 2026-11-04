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
    cd server/frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd server/frontend && npm run build

test-vt-worker:
    cd runtime/vt-worker && npm ci --ignore-scripts --no-audit --no-fund
    cd runtime/vt-worker && npm test

build: build-frontend test-vt-worker
    cargo build --manifest-path server/Cargo.toml --target-dir target --workspace
    cargo build --manifest-path runtime/Cargo.toml --workspace --target-dir target

build-runtime-test-host: build-vt-worker
    cargo build --manifest-path runtime/Cargo.toml --example runtime-test-host --target-dir target

test: test-vt-worker build-runtime-test-host
    if [ "$(uname -s)" = Darwin ]; then TMPDIR="$(node -p 'require("node:fs").realpathSync(require("node:os").tmpdir())')" AOW_RUNTIME_TEST_HOST="$PWD/target/debug/examples/runtime-test-host" cargo test --manifest-path server/Cargo.toml --target-dir target --workspace -- --test-threads=4; else AOW_RUNTIME_TEST_HOST="$PWD/target/debug/examples/runtime-test-host" cargo test --manifest-path server/Cargo.toml --target-dir target --workspace; fi
    cargo test --manifest-path runtime/Cargo.toml --workspace --target-dir target
    cargo test --manifest-path common/Cargo.toml --workspace --target-dir target
    node --test server/git-platform-providers/tests/*.test.mjs
    node --test scripts/tests/*.test.mjs
    cd server/frontend && npm ci --ignore-scripts --no-audit --no-fund
    cd server/frontend && npm run typecheck

clean:
    cargo clean --manifest-path server/Cargo.toml --target-dir target
    cargo clean --manifest-path runtime/Cargo.toml
    rm -rf server/frontend/node_modules server/frontend/dist server/frontend/*.tsbuildinfo
    rm -rf runtime/vt-worker/node_modules

run: build-frontend
    cargo run --manifest-path server/Cargo.toml --target-dir target -p aow-server

run-dev:
    cargo run --manifest-path server/Cargo.toml --target-dir target -p aow-server &
    cd server/frontend && npm run dev

run-runtime-daemon: build-vt-worker
    cargo run --manifest-path runtime/Cargo.toml -p aow-runtime-daemon

build-runtime: build-vt-worker
    cargo build --manifest-path runtime/Cargo.toml --locked

test-runtime: build-vt-worker
    cargo test --manifest-path runtime/Cargo.toml --locked
