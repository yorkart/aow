# Host

This subproject builds `aow-host-daemon`, the service responsible for operations
on its local machine: Agents, ACP conversations, terminals, workspace files and
Git, role environments and individual executions. Server accesses these
capabilities through one authenticated HTTP/WebSocket endpoint.

```text
host/
  Cargo.toml          Independent Rust workspace
  Cargo.lock
  crates/acp-core/    Native ACP SDK, adapter resolution and session model
  crates/acp-service/ Adapter process ownership and native ACP WebSocket service
  crates/agents-core/  Native adapters, discovery, session catalog and completion collection
  crates/agents-service/  Agent management, session HTTP APIs and completion WebSocket
  crates/terminal-core/  PTY lifecycle, typed attachments, replay and VT worker host
  crates/terminal-service/  Terminal HTTP routes and WebSocket transport
  crates/workspace/   Node files, Git, worktrees, search and clipboard storage
  crates/environment/  Role HOME, private identity copies and skills
  crates/exec-core/   Non-interactive Agent executions and process-group ownership
  crates/exec-service/  Single-execution HTTP management
  crates/host-daemon/  Listener, authentication, configuration and shutdown
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
Host owns its execution implementation and build assets. Source builds require
the sibling `../common/` workspace for IDs, wire contracts, SQLite infrastructure
the locale engine and the macOS logging adapter, and require no
Server or host-client source. Compiled Host binaries deploy independently.

The independent caller SDK lives at `../server/crates/host-client`. Server uses that SDK
for management, terminal attachment and native ACP conversations. The business CLI uses `../server/crates/server-client` over authenticated HTTP or
a local Unix socket, independently of execution-node transport. Nodes that invoke
Server business tools need the separately built `aow-cli` binary on PATH. Server
injects the callback URL and a business grant; the node never receives its signing key.

See the [service and deployment reference](../docs/host/host-daemon.md)
for node configuration, authentication, remote Server connection, lifecycle and
API details. The only execution binary is `aow-host-daemon`; acp-service,
terminal-service, agents-service and exec-service are embedded libraries.

Terminal service depends on terminal core. Core exposes a cloneable manager and
typed input/output streams; it owns controller leases, replay boundaries, Agent
operations and final-output draining without depending on HTTP or WebSocket
types. Service maps HTTP requests and WebSocket frames to those APIs. The daemon
owns the core host's lifecycle and mounts service routes at `/v1/terminal` behind
its shared authentication. WebSocket delivery is adapted directly to the core
event sink so a controller takeover stays ordered against flushed output and
acknowledgements. The VT worker bundle remains in `vt-worker/` and is embedded
and managed by terminal core.

## Source boundary checks

`node --test tests/isolation.test.mjs` permits dependencies only on this
subproject and the sibling Common libraries. Caller integration tests use the external `host-test-daemon`
example rather than linking daemon implementation crates:

```sh
cargo build --manifest-path host/Cargo.toml --example host-test-daemon --target-dir target
```

Run that command from the monorepo root before caller integration tests, or set
`AOW_HOST_TEST_DAEMON` to the independently built example executable.

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
a private `runtime/host-client.json` connection descriptor and mirrors node
journals/results. It reconciles unfinished runs after reconnect or Server restart.
The node only enforces the capacity reservation supplied with a single execution.
Installation initialization writes Server configuration; normal startup creates
the default workspace on the configured node. Only one endpoint is configured.

Native Agent traits and their implementations live only in `crates/agents-core`.
Common defines serializable agent IDs, launch requests, capabilities, usage and
opaque session references. Native adapters, execution and Agent storage belong
to Host; Common's SQLite crate supplies generic connections and migrations.

`GET /v1/agents/capabilities` describes node capabilities. Agent argument validation,
profile matching, role layout detection, role prompt preparation and CLI probes use
`/v1/agents/{validate-automation,match-terminal,role-layout,prepare-role-session,probe}`.
`GET /v1/agents/configuration-environment` reads the node's native configuration roots.
All agent launch preparation uses the node's own `AOW_STATE_DIR`.

`POST /v1/terminal/runtimes/{id}/agent-operation` accepts an immutable request ID
and an `initialize` or `submit` operation. Host owns readiness checks, controller
leases and bracketed paste/Enter delivery. Client disconnects do not cancel admitted
operations. Repeated requests with the same ID return the same receipt; conflicting
requests are rejected. `GET .../agent-operation/{request_id}` recovers progress.
Receipts live with the PTY and disappear when it is deleted or the node restarts;
callers must never infer safe resubmission from a missing receipt. A terminal retains
up to 65,536 compact receipts (request fingerprints, without prompt bodies) and rejects new operations once that bound is reached.

Autopilot feedback uses the attach WebSocket `submit` command under its existing
controller lease. Host performs paste and Enter and acknowledges completion.
A forced takeover invalidates that lease before further input; Server retains
only the review workflow and feedback policy.

Agent and execution services follow the same core/service boundary. `agents-core`
retains the capability traits and native implementations, and owns discovery,
launch preparation, session references, completion collection and durable cursors.
`agents-service` adapts these APIs to HTTP and WebSocket. `exec-core` owns one
execution's process lifecycle and output journal; `exec-service` exposes its
management routes. Scheduling and business notifications remain on Server.

`agents-core` observes managed terminals through the `ManagedTerminals` trait,
implemented by `terminal-core` and injected by the daemon. It does not depend on
terminal or execution crates. Bounded one-shot subprocess execution lives in
Host's `process` crate under its `execution` feature, used by Agent probes
and Git provider invocations. Core crates never depend on their service crates;
transport handlers contain no native session databases or process adapters.
Session query contracts are shared through Common and used by Server's SDK.
The existing API paths and node database locations remain unchanged.
