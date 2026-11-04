# Common

Common is an independent Cargo workspace containing libraries shared by Server
and runtime. It has no source dependency on either product. Products may depend
on Common and their own crates; they must not depend on each other's implementation.

```text
common/                       server/                 runtime/
  crates/id/                    crates/runtime-client/  crates/acp/
  crates/protocol/              business APIs and CLI   adapter installation
  crates/runtime-contract/                              and local execution
  crates/acp-client/
  crates/storage/
  crates/filesystem/
  crates/process/
  crates/i18n/
  crates/macos-log/
```

`protocol` owns terminal, filesystem and shared instance wire types.
`runtime-contract` owns shared ACP management, native-session references, workspace
results, execution snapshots, environment data and completion-event envelopes.
It depends only on protocol primitives and serialization/validation libraries;
it has no dependency on either client implementation. Browser projections in the
Server SDK hide node references while preserving the shared wire contract.
`acp-client` uses the official `agent-client-protocol` library for native ACP
messages and provides reusable session state, history, permissions and callbacks.
It accepts a host-supplied transport and cannot start or install an adapter.
Server business crates use only `runtime-client` for ACP interactions.

Node discovery, adapter installation, processes, workspace operations, role HOMEs
and individual execution jobs remain in runtime. Scheduling, authorization and
business data remain in Server. The common filesystem and process crates are
infrastructure primitives; node execution policy belongs to runtime services.

Build either product with `common/` beside its source directory. Shared libraries
are compiled into the relevant binaries; Common is not a service and adds no
runtime deployment dependency. The three workspaces have separate lockfiles.
Update and validate every affected workspace when changing a shared contract.

From this directory:

```sh
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

From the repository root, `node --test scripts/tests/monorepo-isolation.test.mjs`
checks the dependency direction and ACP execution boundary. Rust tests in Server
use an independently built runtime fixture; no daemon implementation is linked
into the Server tests. Shared test adapters are in `fixtures/acp/`.
