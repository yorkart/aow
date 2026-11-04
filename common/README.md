# Common

Common is an independent Cargo workspace for shared data contracts and IDs.
Server and Runtime may depend on it; Common depends on neither product.

- `id`: public identifier generation and encoding.
- `protocol`: terminal, filesystem and instance wire types.
- `runtime-contract`: conversation management requests, responses and events.

Common contains no ACP client engine, adapter discovery, process management,
filesystem operations, persistence, session history or business databases.
Native ACP messages use the official `agent-client-protocol` library in the
Runtime implementation and Server's transport SDK.

Runtime owns conversation execution and durable state. Server owns business
policy and uses `server/crates/runtime-client` to reach the selected node. The
SDK contains transport bookkeeping and delivery cursors, without running a
local session engine or opening a conversation database.

Build either product with its sibling `common/` source directory. Each workspace
has its own lockfile. Compiled products remain independently deployable; Common
is a build dependency and has no service to deploy.

Run `cargo test --manifest-path common/Cargo.toml --workspace` and
`node --test scripts/tests/monorepo-isolation.test.mjs` from the repository root.
