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
  crates/workspace/   Node files, Git, worktrees, search and clipboard storage
  crates/environment/  Role HOME, private identity copies and skills
  crates/executor/    Durable single executions and process-group ownership
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
All source dependencies, protocol types, native agent adapters and build assets
are owned by this directory. It can be copied out of the monorepo and built
without Server, runtime-client or parent-directory crates. Runtime support crates
have distinct package identities from caller-side libraries.

The independent caller SDK lives at `../server/crates/runtime-client`. Server uses that SDK
for management, terminal attachment and native ACP conversations. The business CLI uses `../server/crates/server-client` over authenticated HTTP or
a local Unix socket, independently of execution-node transport. Nodes that invoke
Server business tools need the separately built `aow-cli` binary on PATH. Server
injects the callback URL and a business grant; the node never receives its signing key.

See the [service and deployment reference](../docs/runtime/runtime-daemon.md)
for node configuration, authentication, remote Server connection, lifecycle and
API details. The only execution binary is `aow-runtime-daemon`; acpd and
terminald are embedded libraries.

## Source boundary checks

`node --test tests/isolation.test.mjs` rejects path dependencies outside this
subproject. Caller integration tests use the external `runtime-test-host`
example rather than linking daemon implementation crates:

```sh
cargo build --manifest-path runtime/Cargo.toml --example runtime-test-host --target-dir target
```

Run that command from the monorepo root before caller integration tests, or set
`AOW_RUNTIME_TEST_HOST` to the independently built example executable.

Node agent management adds `POST /v1/agents/discover`, `POST /v1/agents/resolve`
and `POST /v1/agents/prepare-terminal`. These resolve executables against the
node's execution PATH and prepare Codex/Pi assets in node-owned storage.
`PUT /v1/environment` updates the node PATH while preserving other settings.
`GET /v1/environment/discovered-path` discovers the node login-shell PATH afresh.
Read-only native history is available at `POST /v1/agent-sessions/list`,
`POST /v1/agent-sessions/find` and `POST /v1/agent-sessions/snapshot`. Lists and
finds accept node-local `cwd`, optional `home` and configuration `environment`;
find also takes `agent` and native `session_id`. Results include an opaque
node-owned `reference`. Snapshot requests use this reference; callers never
supply transcript paths. The catalog survives node restarts and workspace removal.
`POST /v1/agent-sessions/pane` resolves a managed pane using its actual process
identity and environment. Hosting ownership checks also run on the node.

`GET /v1/agent-events` provides authenticated WebSocket completion events with
persistent sequence numbers and cursor replay. Native transcript readers and
process discovery run autonomously on the node. Server adds business labels,
notification preferences, delivery channels and automation scheduling.

Workspace filesystem, Git, search, watch and clipboard operations are exposed
under `/v1/workspace`. All workspace paths refer to the execution node. Downloads
and uploads stream over HTTP and retain Range, ETag and conditional-write behavior.
Git platform provider scripts execute on the node as bounded subprocesses.

`/v1/environments/roles/{prepare,finish,resolve,probe}` manages private role HOMEs,
identity imports, skills and connection checks. Preparation tickets and decisions
survive daemon restarts. Server publishes role business configuration and then
commits its ticket; reconnecting resolves any unfinished publication. Identity
source paths and `~` expansion refer to the node.

`PUT /v1/executions/{id}` admits one immutable execution specification; GET replays
sequenced events, `/output` reads bounded output chunks, `/cancel` cancels the
process group, and DELETE acknowledges collection. IDs are never reused, including
after acknowledgement or cancellation before admission. Execution journals and
native-session roots live on the node. A Server disconnect does not cancel a job;
node shutdown/restart records an interruption. Requests carry the expected node ID.

Server retains task configuration, schedules, variable rendering, history,
concurrency policy and notification delivery. Its detached automation runner uses
a private `runtime/runtime-client.json` connection descriptor and mirrors node
journals/results. It reconciles unfinished runs after reconnect or Server restart.
The node only enforces the capacity reservation supplied with a single execution.
Installation initialization writes Server configuration; normal startup creates
the default workspace on the configured node. Only one endpoint is configured.
