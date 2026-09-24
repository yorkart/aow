# VT worker

`vt-worker` is a long-running Node.js sidecar that maintains server-side VT
state with `@xterm/headless` and emits ANSI restore streams with
`@xterm/addon-serialize`. It communicates through bounded binary frames on
stdin/stdout. Diagnostics go to stderr.

The checked-in production artifact is a single ESM bundle. It has no runtime
package-install step:

```sh
node dist/vt-worker.mjs
```

Node.js 20 or later is required. For development, run `npm ci` and
`npm run build` to regenerate the bundle. `npm test` rebuilds it, runs
protocol/service tests and a real child-process smoke test. Rust integration
tests use independent workers and temporary daemon sockets.

## Binary protocol v1

Each frame is:

```text
[u32 big-endian body length]
[u32 big-endian metadata length]
[UTF-8 JSON metadata]
[opaque binary payload]
```

Body length includes the metadata-length field, metadata and payload, but
excludes its own four bytes. Metadata contains `protocol_version: 1`.
Lengths are validated before allocating the body. Arbitrary pipe read/write
boundaries have no relation to frame boundaries. Invalid lengths or truncated
frames terminate the worker; the daemon invalidates caches and falls back to
raw replay. Malformed JSON in a complete frame produces a structured error.

This replaces the old NDJSON/base64 protocol. The embedded bundle and daemon
must be built together. A `--vt-worker` override must implement this version;
old overrides fail closed to raw replay. Updating workspace source or building
a bundle does not replace or restart an already running service.

Requests have `id`, `action`, `session_id`, and `generation`. IDs are nonempty
strings or nonnegative JavaScript safe integers. The actions are `create`,
`write`, `resize`, `snapshot`, and `dispose`. Only `write` carries a request
payload; it is the original PTY bytes, with no text decoding or base64.

| Action | Successful response | Execution semantics |
| --- | --- | --- |
| create | Yes | Allocate a session at offset zero |
| write | **None** | Apply binary bytes, then advance worker-local applied offset |
| resize | **None** | Apply after preceding writes finish parsing |
| snapshot | Metadata plus binary ANSI payload | Barrier after preceding writes/resizes |
| dispose | Yes | Barrier that releases session state |

All operations execute in stream order. A write completion callback updates
only worker-local state; it does not emit an ACK. The daemon sends further
writes without waiting for parse completion. Neither side treats acceptance
into a pipe as proof that xterm has parsed the data.

A successful query/lifecycle response preserves its `id` and `action`:

```json
{"protocol_version":1,"id":3,"ok":true,"action":"snapshot","result":{"session_id":"pane-1","generation":"epoch-1","applied_offset":5,"cols":80,"rows":24,"lines":["hello"],"byte_length":5}}
```

This illustrative response has a five-byte binary payload `hello`; actual
`lines` contains exactly `rows` strings and ANSI payloads may include escape
sequences. Binary data is never included in JSON.

Every failed operation produces an error, including one-way writes/resizes:

```json
{"protocol_version":1,"id":2,"ok":false,"action":"write","session_id":"pane-1","generation":"epoch-1","error":{"code":"offset_mismatch","message":"gap"}}
```

Mutation failures identify the session and generation. An independent daemon
reader consumes these even while the actor is idle or waiting for pipe
capacity, invalidates only the matching session, and lets it use raw replay.
Late errors for a stale generation cannot disable a different generation.
Protocol/transport failures invalidate the worker's sessions. No automatic
resend attempts to reconstruct a partially applied VT stream.

## Actions

The following examples show only JSON metadata; the framing encoder adds
`protocol_version: 1`.

### create

```json
{"id":1,"action":"create","session_id":"pane-1","generation":"epoch-1","cols":80,"rows":24,"scrollback":100,"initial_offset":0}
```

`initial_offset` is required and must be zero. Scrollback is between 0 and 100
lines, defaulting to 100. Duplicate live session IDs are rejected.
Requested dimensions must be between 1 and 1,000. A one-column request is
normalized to two columns, matching xterm's minimum width for wide characters.
Create responses, snapshots and screen-cell accounting use this effective size.

### write

```json
{"id":2,"action":"write","session_id":"pane-1","generation":"epoch-1","start_offset":0}
```

The frame payload contains raw PTY bytes. `start_offset` must match the
worker's applied offset. The worker awaits xterm's callback before applying
later operations or advancing that offset. Gaps, duplicates, stale generations
and unsafe-integer offsets are rejected. Success emits nothing.

### resize

```json
{"id":3,"action":"resize","session_id":"pane-1","generation":"epoch-1","at_offset":5,"cols":120,"rows":40}
```

No payload. `at_offset` must match the worker's applied offset. The daemon
invalidates its old-size cache synchronously before queueing the resize, and
prevents batches from crossing the geometry boundary. Success emits nothing.
One-column requests use the same two-column normalization as `create`; the
daemon validates snapshots against the normalized dimensions.

### snapshot

```json
{"id":4,"action":"snapshot","session_id":"pane-1","generation":"epoch-1"}
```

Returns generation, applied offset, dimensions, viewport `lines`, and
`byte_length` in metadata; the ANSI restore stream is the binary payload.
Optional `scrollback` can request fewer history lines. Viewport lines trim
trailing spaces, exclude scrollback, and follow the active normal/alternate
screen. Agent startup probes use them to detect current input readiness.

The daemon validates the result's applied offset and dimensions against the
ordered writes/resizes it sent, plus the existing generation/revision barrier,
without needing per-write or per-resize success replies. Only validated results
enter the cache. Stream recovery then combines this snapshot with raw output
after its applied offset.

### dispose

```json
{"id":5,"action":"dispose","session_id":"pane-1","generation":"epoch-1"}
```

Returns `disposed: true` and the final applied offset after earlier operations
complete. SIGTERM/SIGINT stop reading, dispose sessions and exit. The caller
owns the PTY lifecycle; worker restarts do not retain VT state.

## Bounds and backpressure

- Frame body: 16 MiB + 64 KiB. Request metadata: at most 64 KiB.
- Binary write payload: at most 8 MiB at the protocol boundary; terminald sends
  coalesced batches of at most 64 KiB.
- Binary snapshot payload: at most 2 MiB. The daemon separately bounds viewport
  text, per-session cache size and aggregate cached bytes.
- Requested dimensions: 1–1,000 each; effective width is at least 2 columns.
  At most 1,000,000 effective screen cells per session and 8,000,000 across the
  worker. Live VT sessions: at most 256.
- Session IDs, generations, string request IDs and actions have separate
  metadata byte limits.

The daemon retains its 2,048-command queue and 16 MiB budget for queued and
currently-being-written raw bytes. A batch releases its reservation once the
pipe accepts it, not after an application ACK. Additional transport storage is
bounded by pipe capacity, stream buffering and the worker's single bounded
frame. The worker reads and completes one operation before consuming the next;
it does not eagerly drain stdin into an unbounded promise queue.

Consecutive writes coalesce only within the same session, contiguous offset
range and geometry revision. No timer delays idle output. Exceeding the command
or byte budget degrades the affected session instead of blocking PTY output.
A bounded write/query timeout handles a stalled worker. Shutdown cancels these
waits and kills/reaps only the child owned by that worker instance.
