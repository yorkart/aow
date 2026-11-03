# Runtime daemon

`aow-runtime-daemon` is a standalone node service. One Rust process embeds the
`aow-acpd` and `aow-terminald` libraries and owns a single TCP HTTP listener,
configuration, authentication and shutdown sequence. ACP adapters, PTY shells
and the optional Node.js VT worker are child processes; it does not launch
separate `aow-acpd` or `aow-terminald` daemons.

This phase delivers the node service. AoW server node registration, routing a
conversation to a remote node, and the corresponding management UI remain
separate work. Existing server installations continue using their local daemon
clients until that integration is implemented.

## Build and start

```sh
npm --prefix vt-worker ci --ignore-scripts --no-audit --no-fund
npm --prefix vt-worker run build
cargo build --release -p aow-runtime-daemon
./target/release/aow-runtime-daemon --state-dir "$HOME/.aow" --listen 127.0.0.1:9760
```

The release archive also includes `bin/aow-runtime-daemon`. The existing installer
keeps its current local services; installing an archive does not activate a node
service automatically. For a dedicated Linux node, install this binary at
`~/.local/bin/aow-runtime-daemon` and the supplied
`packaging/systemd/aow-runtime-daemon.service` as a user unit. On macOS the same
binary can run under a process supervisor with the arguments above.

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
| `data/acp/` | Existing AoW ACP history and exclusive ownership lock |
| `cache/acp/` | Existing adapter and Registry cache |
| `data/runtime-daemon/node-id` | Stable UUID identifying this installation |
| `runtime/runtime-daemon/token` | Automatically generated private bearer token |

Passing the existing state root retains its data and configuration. The node
service and legacy `aow-acpd` cannot own the same ACP state directory
simultaneously; stop the old ACP service before starting a node on that root.
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
| `GET /v1/acp/stored-sessions` | Metadata from the existing AoW ACP history store |
| `GET /v1/acp/stored-sessions/{id}` | Existing AoW transcript snapshot, without starting an adapter |
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

Existing AoW history remains available through `stored-sessions` and its snapshot endpoint; it is not
converted into ACP messages or automatically supplied as adapter history.
Native session storage and `session/list`/`session/load` remain adapter-owned.
New raw ACP conversations do not create AoW facade transcript snapshots; the
future server node integration must maintain its own conversation records.

## Terminal WebSocket

`GET /v1/terminal/runtimes/{id}/attach` uses the existing terminald protocol:
binary frames carry terminal input/output; JSON frames carry control, stream
metadata, resize, status and acknowledgements. Existing query options such as
`control=v2`, `observer=v1`, `epoch`, `after`, `capabilities=vt-snapshot-v1` and
`snapshot_compression=gzip` retain their behavior. See the types in
`crates/protocol/src/terminal/attach.rs` for the frame schema.

Unlike ACP transports, closing a terminal attachment leaves the PTY running.
Reattach using its epoch and offset to replay available output. Terminal
instances and their replay buffers live in daemon memory, and stop on DELETE
or daemon shutdown. SIGINT/SIGTERM stops accepting connections, closes runtime
admission, checkpoints the existing ACP store and reaps both runtime types.
