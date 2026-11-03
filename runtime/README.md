# Runtime

This subproject builds `aow-runtime-daemon`, one process that hosts ACP adapters
and terminal instances behind an authenticated HTTP/WebSocket endpoint.

```text
runtime/
  Cargo.toml          Independent Rust workspace
  Cargo.lock
  crates/acp/         Native ACP SDK, adapter resolution and session model
  crates/acpd/        Adapter process ownership and native ACP WebSocket service
  crates/terminald/   PTY lifecycle, replay and VT worker host
  crates/runtime-daemon/  Listener, authentication, configuration and shutdown
  vt-worker/         Bundled Node.js VT parser
```

From this directory:

```sh
just build
just test
just run --state-dir "$HOME/.aow" --listen 127.0.0.1:9760
```

Without just, build the worker with `npm --prefix vt-worker ci --ignore-scripts
--no-audit --no-fund` and `npm --prefix vt-worker run build`, then run
`cargo build --workspace --locked` or `cargo test --workspace --locked`.
Rust shared infrastructure remains in the parent repository; this is a monorepo
subproject, not a separately vendored source tree. It does not depend on Server
or runtime-client.

The shared caller SDK lives at `../crates/runtime-client`. Server uses that SDK
for management, terminal attachment and native ACP conversations. The local
CLI-to-Server business socket uses `../crates/local-client` and is independent
of execution-node transport.

See the [service and deployment reference](../docs/runtime/runtime-daemon.md)
for node configuration, authentication, remote Server connection, lifecycle and
API details. The only execution binary is `aow-runtime-daemon`; acpd and
terminald are embedded libraries.
