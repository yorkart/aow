# ACP browser synchronization

ACP reads share the page's `/api/events/ws` connection with workspace and operation
notifications. HTTP writes and `/api/acp/sessions/{thread_id}/control` retain their
existing authorization and lifecycle. Subscribing never claims control, starts an
adapter, resumes a task, or sends a prompt.

## Protocol version 1

The server sends `{"event":"acp","data":{"type":"hello","protocol":1}}`.
Clients then send one command per resource:

```json
{"action":"subscribe","id":"1","resource":{"kind":"session","thread_id":"thread"}}
```

`{"action":"unsubscribe","id":"1"}` releases that subscription. IDs are local
to the socket; reconnect creates new IDs. The server permits 256 subscriptions and
8 KiB commands per socket. Existing Origin, login and session revocation checks
apply to the entire socket.

A `ready` envelope contains `id` and `head`. A `change` envelope contains `ids` and
`head`. The head always includes `resource`, `boot_id`, `version`, `deleted` and
`urgent`. `boot_id` identifies the server's event epoch; versions are monotonic
within that epoch. They are opaque resource invalidation versions, separate from
session transcript revisions. Clients ignore older or duplicate heads.

| Resource | Identity | Read after invalidation |
| --- | --- | --- |
| `session` | `thread_id` | Existing overview snapshot / updates API |
| `sessions` | `cwd` | Existing workspace session list |
| `connection` | `thread_id` | Existing session connection API, including null bindings |
| `requests` | `connection_id` | Existing pending request list; empty replaces the previous list |
| `operation` | `agent_id`, nullable `cwd` | Installation status when cwd is null, otherwise connection / restore status |

All five resource types use **version notifications**, not embedded authoritative
content snapshots. Session heads additionally carry `session.generation`,
`session.revision` and `session.info` for hidden-view metadata. Operation status
and its notification include an `operation_id` distinguishing attempts, and status
retains its final result.
Expanded process bodies use the overview's turn revision and share a scoped read
cache; collapsing the last consumer releases that cache and cancels its read.

## Ordering and recovery

The broadcast receiver exists before upgrade. The server registers a subscription
before reading its head and acknowledging `ready`. The client waits for that
acknowledgement before fetching, retaining newer heads while a read is in flight.
Only one read per resource runs at a time, and a dirty resource is read again when
necessary. Session generation changes invalidate in-flight responses and select
snapshot recovery; expired history continues to use the existing snapshot result.

Mutations publish after committing their visible state. Stream notifications are
coalesced for up to 50 ms and list invalidations for up to 500 ms. Permission,
connection, progress and terminal session states bypass stream coalescing.

The broadcast buffer is bounded to 512 events, and each socket has at most one
pending notification per subscribed resource. A lagged receiver sends `resync`
and disconnects. Socket sends time out after five seconds. Reconnect uses the
shared transport's exponential backoff and restores subscriptions, obtaining new
heads even if nothing changes after reconnect. Deleted sessions and invalid
connections terminate subscriptions; authentication revocation also clears cached
ACP data and prevents reconnect attempts until a new authenticated page lifecycle.

A healthy connection has no periodic ACP HTTP reads. A server that opens the
socket but sends no v1 greeting within two seconds explicitly enters `degraded`
compatibility mode with one shared 30-second fallback timer. Ordinary transport
failures retain cached data and reconnect without starting that timer. A v1
greeting immediately cancels fallback. HTTP failures retry only outstanding dirty
resources with bounded exponential backoff.

## Validation

`npm --prefix frontend run test:acp` includes browser request-count tests with
60 seconds of browser-clock advancement, snapshot races, repeated notifications,
hidden views, remounts, generation changes, reconnect, deletion, revoked login and
legacy fallback. The existing component suite covers authentication, ownership,
progress, imports, conversation rendering and scoped process loading.

Rust tests exercise committed event versions, bounded queues, progress results,
subscription readiness, reconnect, deletion and coalescing. Run `cargo test
--workspace`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
`cargo fmt --all -- --check`, frontend typecheck/build and repository JavaScript
lint. Installation/release script regression tests are outside this change.
