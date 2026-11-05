# Common

Common is an independent Cargo workspace for shared data contracts, IDs and
generic infrastructure.
Server and Runtime may depend on it; Common depends on neither product.

- `i18n`: locale context, HTTP language negotiation and interpolation; product catalogs remain in each product.
- `id`: public identifier generation and encoding.
- `macos-log`: the macOS Unified Logging adapter shared by Server and Runtime.
- `protocol`: terminal, filesystem and instance wire types.
- `runtime-contract`: ACP management, agents, workspace operations, role environments and single-execution requests, responses and events.
- `sqlite`: private owned-database connections, external read-only connections and
  transactional migration infrastructure (`aow-sqlite`).

Common contains no ACP client engine, adapter discovery, process management,
workspace filesystem operations, session history or business database schemas.
Native ACP messages use the official `agent-client-protocol` library in the
Runtime implementation and Server's transport SDK.

SQLite callers provide database paths, migration domain names and migration SQL.
The shared crate owns only connection configuration and the generic
`schema_migrations` bookkeeping table. Server's `db::tasks` and Runtime's
`db::{acp,codex_like,hermes}` retain their schemas, queries and business transactions.
Both products continue to own separate databases. Read-only connections never
create or migrate external databases. The extracted SQLite crate retains the
original code's GPL-3.0-or-later license.

The shared macOS adapter connects tracing, panic reports and startup errors to
Unified Logging. Each process supplies its category and default filter. Native
code is compiled only for macOS; the crate has no platform API on other hosts.
Operation history and business audit rules remain in Server.

Runtime owns conversation execution and durable state. Server owns business
policy and uses `server/crates/runtime-client` to reach the selected node. The
SDK contains transport bookkeeping and delivery cursors, without running a
local session engine or opening a conversation database.

Build either product with its sibling `common/` source directory. Each workspace
has its own lockfile. Compiled products remain independently deployable; Common
is a build dependency and has no service to deploy.

Run `cargo test --manifest-path common/Cargo.toml --workspace` and
`node --test scripts/tests/monorepo-isolation.test.mjs` from the repository root.
