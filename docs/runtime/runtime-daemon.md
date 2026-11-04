# Runtime daemon

`aow-runtime-daemon` is a standalone node service. One Rust process embeds the
`aow-acp-service` and `aow-terminal-service` libraries and owns a single TCP HTTP listener,
configuration, authentication and shutdown sequence. ACP adapters, PTY shells
and the optional Node.js VT worker are child processes; it does not launch
separate `aow-acpd` or `aow-terminald` daemons.

AoW Server uses the shared `server/crates/runtime-client` SDK for all execution-node
operations. HTTP manages instances and configuration; WebSocket carries native
ACP JSON-RPC and terminal streams. The execution modules and VT worker live in
the independent [runtime subproject](../../runtime/README.md), with its own Cargo
workspace and lockfile. The caller SDK stays in the Server workspace. Both products reference the sibling
Common workspace for data contracts and IDs.

`aow-terminal-core` owns PTYs, controller leases, replay, native Agent operations
and the VT worker. It exposes typed commands and events without an HTTP/WebSocket
dependency. `aow-terminal-service` provides routes, JSON conversion, HTTP errors
and WebSocket transport; daemon startup and shutdown own the underlying core host.
Terminal routes and wire messages are unchanged, including the `aow-terminald`
health service identifier.

## Build and start

```sh
npm --prefix runtime/vt-worker ci --ignore-scripts --no-audit --no-fund
npm --prefix runtime/vt-worker run build
cargo build --manifest-path runtime/Cargo.toml --release -p aow-runtime-daemon
./runtime/target/release/aow-runtime-daemon --state-dir "$HOME/.aow" --listen 127.0.0.1:9760
```

The release archive includes `bin/aow-runtime-daemon`. First installation activates
runtime-daemon before Server to create its token. Updates restart runtime last
so that activation can finish even when it closes the installer’s PTY. `--skip-runtime` updates Server while retaining the
current execution node. The retired acpd/terminald executables, sockets and
client crates are removed.

To connect Server to another machine, configure its `AOW_RUNTIME_URL` (HTTP or
HTTPS) and `AOW_RUNTIME_TOKEN_FILE` (a local file containing that node's token),
or pass `--runtime-url` and `--runtime-token-file`. For example:

```sh
AOW_RUNTIME_URL=https://node.example.com \
AOW_RUNTIME_TOKEN_FILE=/private/path/node-token \
cargo run -p aow-server
```

Server currently selects one configured endpoint. Multi-node registration,
per-project routing and a node management UI are separate work. Workspace paths,
role homes and adapter executables must exist on the selected node; this change
does not synchronize project checkouts between machines.

`--state-dir` must be absolute. It defaults to `AOW_STATE_DIR`, then `~/.aow`.
The process reads ACP agent definitions from `STATE_DIR/aow-acp.json` and the
execution PATH from `STATE_DIR/aow-settings.json`, using the existing formats.
Both custom commands and Registry adapters use the existing ACP resolver and
cache. Registry adapters are prepared when an instance is created.

The state root contains:

| Path | Purpose |
| --- | --- |
| `aow-acp.json` | Existing ACP agent definitions |
| `aow-settings.json` | Existing execution environment settings |
| `runtime-daemon.json` | Optional node transport configuration |
| `data/runtime-daemon/daemon.lock` | Exclusive node ownership lock |
| `cache/acp/` | Existing adapter and Registry cache |
| `data/runtime-daemon/node-id` | Stable UUID identifying this installation |
| `runtime/runtime-daemon/token` | Automatically generated private bearer token |

The daemon owns configuration, adapter cache and conversation persistence.
Conversation metadata and snapshots live in the node's `STATE_DIR/data/acp/`.
Server never opens that database. Its SDK reads node-owned snapshots through
HTTP and sends native ACP messages through WebSocket. Server stores only business
associations and notification delivery cursors. No legacy history migration is
provided.
Terminals receive `AOW_STATE_DIR` as a default child environment value; the
terminal creation specification can supply explicit environment overrides.

## Transport configuration

By default the daemon reads `STATE_DIR/runtime-daemon.json` when present.
`--config /path/to/node.json` chooses another configuration file; an explicitly
selected missing or invalid file fails startup. For example:

```json
{
  "listen": "127.0.0.1:9760",
  "token_file": "node-token",
  "allowed_origins": [],
  "vt_node": "/usr/local/bin/node"
}
```

Supported fields are `listen`, `token_file`, `allowed_origins`, `vt_node` and
`vt_worker`. Unknown fields are rejected. Relative `token_file` and `vt_worker`
paths are relative to the configuration file. The CLI flags `--listen`,
`--token-file`, `--vt-node` and `--vt-worker` override those settings. A bare
`vt_node` command is resolved through the process PATH; its default is `node`.
Node.js 20+ enables the embedded VT worker. If Node is unavailable, terminal
instances use the existing raw replay mode.

Every HTTP request and WebSocket upgrade, including health, requires
`Authorization: Bearer TOKEN`. With no explicit token file, the daemon creates a
random token at `runtime/runtime-daemon/token` and reuses it on restart. An explicit
token file must already exist. Tokens must have 32–4096 non-whitespace ASCII
characters; files must be owned by the runtime user, regular files, and mode
`0600` or stricter. Tokens are never printed in startup logs or placed in URLs.
Restart the daemon after rotating the token file.

Server clients normally omit `Origin`. If it is present, it must exactly match
one of `allowed_origins`; the default rejects browser origins. Authentication
still applies to allowed origins. The service offers HTTP/WS directly. For
remote deployment, terminate HTTPS/WSS at a reverse proxy, forwarding the
Authorization header and WebSocket upgrades to the node listener. Configure
that proxy's listener and certificates independently.

## Management API

All working directories and executable paths refer to the node's filesystem.
The token grants control over processes running as the daemon's OS user.

| Method and path | Behavior |
| --- | --- |
| `GET /v1/health` | Service version, stable `node_id`, boot `instance_id`, PID and capabilities |
| `GET /v1/acp/agents` | Configured ACP adapters and preparation state |
| `GET /v1/acp/registry` | ACP registry catalog |
| `POST /v1/acp/agents/{id}/install` | Prepare an adapter on the node |
| `GET /v1/acp/agents/{id}/installation` | Preparation status |
| `GET/PUT /v1/acp/settings` | Node ACP settings with optimistic revision checking |
| `GET/PUT /v1/environment` | Read/update node execution PATH |
| `GET /v1/environment/discovered-path` | Rediscover PATH using the node login shell |
| `POST /v1/agents/discover`, `POST /v1/agents/resolve` | Discover/resolve executables on the node |
| `POST /v1/agents/prepare-terminal` | Prepare node-owned resume flags and Codex/Pi launch assets |
| `POST /v1/agent-sessions/list`, `POST /v1/agent-sessions/find` | Native history metadata and opaque node references |
| `POST /v1/agent-sessions/snapshot` | Read a transcript by node-owned reference |
| `POST /v1/agent-sessions/pane` | Native sessions associated with a managed pane |
| `POST /v1/agent-sessions/process-check` | Validate pane ownership against process birth identity |
| `POST /v1/agent-sessions/resolve-worktree` | Validate a worktree against registered repository roots on the node |
| `POST /v1/directories/resolve` | Resolve an existing node workspace |
| `POST /v1/files/read`, `POST /v1/files/write` | Node filesystem callbacks |
| `PUT /v1/workspaces/removing` | Fence workspace access and reap its ACP processes |
| `POST /v1/workspaces/recover` | Recover stale workspace fences before reconnecting |
| `GET /v1/acp/instances` | Managed ACP processes and their status |
| `PUT /v1/acp/instances/{id}` | Create an adapter process using `{ "agent_id": "...", "cwd": "/repo" }` |
| `GET /v1/acp/instances/{id}` | Process PID, specification, status and WebSocket path |
| `DELETE /v1/acp/instances/{id}` | Stop and reap its process group; remove the management entry |
| `GET /v1/terminal/health` | Embedded terminal runtime health, with the same daemon PID |
| `GET /v1/terminal/agents` | Terminal agent discovery |
| `GET /v1/terminal/runtimes` | Terminal instances |
| `PUT /v1/terminal/runtimes/{id}` | Create a PTY using the existing `TerminalRuntimeSpec` |
| `GET /v1/terminal/runtimes/{id}` | PTY status and geometry |
| `DELETE /v1/terminal/runtimes/{id}` | Delete the PTY and reap its child |
| `GET /v1/terminal/runtimes/{id}/screen` | VT viewport snapshot when available |

ACP IDs accept 1–128 ASCII letters, digits, underscores, dots or hyphens.
The IDs `.` and `..` are excluded. Creation returns `201`; repeating the same ID and specification returns `200`
with the same process. A conflicting specification returns `409`. Exited
instances remain queryable until deleted. At most 256 managed ACP entries are
retained; remove unused entries before creating more. DELETE is idempotent.
Once admitted, managed create/delete operations finish even if the HTTP caller
disconnects. ACP creation also accepts an optional `execution_environment` object
with an existing absolute `home` directory and an `env` map for role-specific
process overrides. Use `home` to set HOME; an `env.HOME` entry is rejected.
Environment values are never echoed by the management API. These paths belong
to the node, and the daemon does not create role homes on behalf of the server.

Example health and ACP creation:

```sh
NODE_TOKEN=$(cat "$HOME/.aow/runtime/runtime-daemon/token")
curl --fail --header "Authorization: Bearer $NODE_TOKEN" \
  http://127.0.0.1:9760/v1/health
curl --fail --request PUT --header "Authorization: Bearer $NODE_TOKEN" \
  --header 'Content-Type: application/json' \
  --data '{"agent_id":"my-agent","cwd":"/absolute/node/workspace"}' \
  http://127.0.0.1:9760/v1/acp/instances/example
```

## Native history and completion events

Native session metadata contains a `reference` with `node_id`, an opaque `handle`
and session identity. Server persists this reference for shares and business
associations; transcript paths and trusted history roots remain in the node's
SQLite catalog. `POST /v1/agent-sessions/snapshot` accepts `{ "reference": ... }`.
The node verifies the handle and identity, then rechecks the trusted transcript
root. A deleted worktree does not prevent reading its retained history. Unknown,
mismatched or unavailable references return `410`; a reference cannot be moved
to a different node. An unavailable runtime never causes Server to read local
history. Server builds exclude the in-process history fixtures used by tests.

`GET /v1/agent-events` upgrades to an authenticated WebSocket. The first frame is
`{ "type": "ready", "node_id": "...", "head": 42, "oldest": 1, "after": 42 }`.
Without a cursor, a subscription starts at the current head and receives future
events. Reconnect with `?node_id=...&after=42` to replay later records. Completion
frames contain `type: "completion"`, `node_id`, `sequence`, and an `event` with
agent/session identity, current title, conclusion, token usage and owning pane
IDs. Business labels and notification delivery policy belong to Server.

The node runs bounded native transcript readers independently of Server
connections. Existing history starts at EOF; exited CLI readers receive a final
poll. Completions are committed to a node-local SQLite log before transmission,
and the latest 4096 records survive node restarts. WebSockets exchange heartbeats
and reconnect after a lost connection. Server persists its cursor in
`state/completion-cursor.json` after dispatch and ignores replayed sequence IDs.
Transport replay is at least once: a Server crash between dispatch and cursor
persistence can repeat that delivery; notification providers are not part of the
cursor transaction.

A different node ID returns `409`. An expired or future cursor produces an
explicit `gap` frame; Server logs the error and stops that listener rather than
silently skipping records. After inspecting such a gap or intentionally replacing
a node, an operator can remove Server's cursor file and restart Server to start
from the new head. This deliberately abandons unconsumed history.

## Workspace, role environments and executions

All filesystem paths in these APIs belong to the configured runtime node. Server
keeps project, role and task business metadata, and uses runtime-client for node
operations. There are no path dependencies between the two source workspaces.

| API | Purpose |
| --- | --- |
| `POST /v1/workspace/operations` | Filesystem metadata/mutations and Git/worktree operations |
| `GET/PUT /v1/workspace/raw` | Streaming transfer, Range and conditional writes |
| `POST /v1/workspace/search` | Bounded search with a caller request ID |
| `DELETE /v1/workspace/search/{id}` | Cancel a pending or active search |
| `GET /v1/workspace/watch` | WebSocket Git/worktree changes; send the root list first |
| `POST /v1/workspace/clipboard` | Validate and store temporary images on the node |
| `POST /v1/workspace/provider` | Execute a protocol-v4 Git provider on the node |
| `POST /v1/environments/identity-sources` | Discover node-local identity presets |
| `POST /v1/environments/roles/prepare` | Prepare HOME/identity/skills under a durable ticket |
| `POST /v1/environments/roles/finish` | Commit or roll back a prepared ticket |
| `POST /v1/environments/roles/resolve` | Reconcile published configuration and resolve environment |
| `POST /v1/environments/roles/probe` | Check the CLI or native ACP adapter in that environment |
| `PUT /v1/executions/{id}` | Admit one immutable execution request |
| `GET /v1/executions/{id}` | Replay events after a sequence cursor and read its result |
| `GET /v1/executions/{id}/output` | Read a bounded stdout/stderr chunk by offset |
| `POST /v1/executions/{id}/cancel` | Cancel the entire execution process group |
| `DELETE /v1/executions/{id}` | Acknowledge collection and remove execution files |

Role environment preparation imports credentials as private independent files.
Server commits after publishing its business configuration; durable tickets and
decisions reconcile interrupted publication without depending on the source Home
still existing. Skill imports reject escaping/cyclic symlinks. Temporary clipboard
images have a 10 MiB per-image limit, a 256 MiB node quota and a 24-hour lifetime.

An execution contains a workspace configuration, resolved launch description,
rendered prompt, environment overrides and an optional opaque capacity reservation.
Scheduling, prompt variables, task revision/history, notification preferences and
concurrency policy stay on Server. The node owns worktree preparation/cleanup,
agent flags/processes, native session discovery and final result capture. It does
not schedule tasks or read Server's task database.

Execution IDs are idempotent and permanently retired after acknowledgement. An
early cancellation also retires its ID, preventing delayed submission from starting
it. The caller supplies the expected stable node ID. A disconnected or killed
Server runner leaves the node job running; Server reconciles journals, output and
results after reconnect. Node shutdown cancels active jobs, while startup marks
unfinished durable jobs interrupted after reaping their recorded process groups.

The detached Server automation runner reads a private connection descriptor at
`<server-state>/runtime/runtime-client.json`. It persists submission intent before
sending it and retries with the same ID. Server mirrors completion before
acknowledging node cleanup. Installation `--initialize-state` creates business
configuration only; normal startup initializes default workspaces on the node.
This stage supports one endpoint and does not retain the old local execution path.

Managed ACP creation also accepts a bounded `environment` map (128 keys and
128 KiB total) for caller-supplied process variables. The node applies an explicit
role `execution_environment` afterward and assigns managed identity variables
itself. Server uses this generic mechanism for business callback credentials;
no Server business types or signing keys are needed by runtime.

## ACP WebSocket

There are two connection entry points:

- `GET /v1/acp/instances/{id}/acp` upgrades an explicitly managed process.
- `GET /acp?agent_id=my-agent&cwd=%2Fabsolute%2Fnode%2Fworkspace` creates a
  connection-owned process, upgrades it, and removes its management entry when
  the connection ends. URL-encode query parameters.

The upgrade response includes `Acp-Connection-Id`. One process has one controller;
a second attachment returns `409`. Each text frame carries one native ACP
JSON-RPC message. Start with `initialize`, then use `session/new`,
`session/load`, `session/prompt`, `session/cancel`, etc., according to the
adapter's negotiated capabilities. Responses, notifications, permission
requests, filesystem/terminal callbacks and extensions travel bidirectionally.
Unknown methods and fields pass through. The daemon does not initialize on the
client's behalf or answer client-bound ACP requests; the controlling ACP client
must implement its advertised capabilities.

No AoW facade envelope or private method mapping is added. Input JSON whitespace
is normalized only to encode a single stdio line. Batches are rejected with
JSON-RPC `-32600`, malformed JSON with `-32700`, and binary frames are ignored.
Messages are limited to 16 MiB and queues are bounded. The connection sends
ping frames every 15 seconds and closes an unresponsive peer after 45 seconds;
slow transport writes time out after five seconds.

The implementation follows the WebSocket shape in the active
[ACP remote transport proposal](https://agentclientprotocol.com/rfds/streamable-http-websocket-transport).
It provides the WebSocket profile, not Streamable HTTP/SSE. The proposal is
still evolving; this does not claim support for its future replay guarantees.

ACP connection lifetime is the WebSocket lifetime. Disconnect, process exit,
DELETE or node shutdown ends the connection and reaps the adapter process group.
A new connection starts with `initialize`; use native `session/load` with the
adapter's saved session ID when supported. In-flight prompts, pending requests
and transport frames are not persisted or replayed across disconnect or daemon
restart. Managed creation without attachment leaves the adapter waiting until
DELETE or shutdown.

### Durable conversation service

Server uses the node-owned conversation service rather than attaching directly
to an adapter process:

- `POST /v1/acp/conversations`: typed management operations from
  `common/crates/runtime-contract`, including identities, history, snapshots,
  installation and workspace fences.
- `GET /v1/acp/conversations/acp`: native ACP JSON-RPC, using the official ACP
  schema. Prompts, cancellation, modes, configuration and permission responses
  travel on this channel. AoW admission notifications and steering use namespaced
  extensions; prompt replies retain ACP stop reasons.
- `GET /v1/acp/conversations/events`: typed invalidations and durable turn
  completion events. Journal identity and sequence cursors permit reconnect
  replay. Server acknowledges business delivery in its own state directory.

The node handles adapter filesystem callbacks, maintains session state and owns
prompt admission. Closing a Server connection does not stop admitted work.
Permission requests remain on the node and can be answered after reconnect.
After a daemon restart, history and admission receipts remain available; live
adapter transports must be resumed according to their capabilities. Retrying an
already admitted request ID does not execute another prompt.

The raw adapter endpoints above retain their connection-owned lifecycle for
standalone ACP clients. They do not provide the durable conversation service.

## Terminal WebSocket

`GET /v1/terminal/runtimes/{id}/attach` uses the existing terminald protocol:
binary frames carry terminal input/output; JSON frames carry control, stream
metadata, resize, status and acknowledgements. Existing query options such as
`control=v2`, `observer=v1`, `epoch`, `after`, `capabilities=vt-snapshot-v1` and
`snapshot_compression=gzip` retain their behavior. See the types in
`common/crates/protocol/src/terminal/attach.rs` for the frame schema.

Unlike ACP transports, closing a terminal attachment leaves the PTY running.
Reattach using its epoch and offset to replay available output. Terminal
instances and their replay buffers live in daemon memory, and stop on DELETE
or daemon shutdown. SIGINT/SIGTERM stops accepting connections, closes runtime
admission and reaps both runtime types.
