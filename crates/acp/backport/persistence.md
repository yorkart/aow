# ACP SQLite persistence map

Baseline: Zed `dc3fb21676457b84d2233ac4c6bec5cebc698ec3` (2026-10-08).
Keep this baseline and the source hashes in `sources.json` together when backporting.

## Upstream boundary

Zed's `agent_ui::ThreadMetadataStore` persists external ACP thread metadata in
SQLite's `sidebar_threads`. The external agent owns its transcript and replays it
through `session/load`. Zed's native agent has a separate `agent::ThreadsDatabase`
for its own versioned, serialized thread bodies. These are different responsibilities.

AoW ports the metadata-store structure and preserves its existing offline display
and resume-only-agent support with an **AoW-only snapshot cache**. Both now live in
`<state directory>/acp/threads.sqlite`. A cached body is a versioned JSON blob
inside SQLite, following the serialization boundary in `agent::db`; it is not a
standalone JSON file or a log of every protocol notification. Imported sessions
have only metadata and an empty `needs_load` placeholder until explicitly opened.

## Code correspondence

Upstream paths are relative to the Zed repository; local paths are relative to
`crates/acp/src` unless prefixed with `crates/`.

| Upstream | Local counterpart | Preserved shape / host adaptation |
| --- | --- | --- |
| `crates/agent_ui/src/thread_metadata_store.rs::ThreadMetadata`, `ThreadMetadataStore` | `agent_ui/thread_metadata_store/mod.rs` | Metadata cache, lookup, save, reload and delete. GPUI subscriptions become reducer calls; queued saves carry live session handles. |
| `ThreadMetadataDb::{MIGRATIONS, LIST_QUERY, list, save, delete}` | `crates/db/src/acp/sidebar_threads/`, `crates/db/src/acp/migrations/`; `agent_ui/thread_metadata_store/record.rs` | Table SQL and row mapping live in `aow-db`; ACP converts stored columns to its metadata model. Existing schemas and migration domains are preserved. |
| `DbOperation`, `dedup_db_operations`, background operation drain | `agent_ui/thread_metadata_store/operations.rs` and `mod.rs` | Latest upsert/delete per thread wins; the single writer captures the referenced session and commits its metadata and cached body atomically. |
| `crates/db`, `sqlez::Connection::migrate` | `crates/db/src/sqlite/`, `crates/db/src/acp/mod.rs` | Private SQLite connections and domain/version migrations; the ACP data source owns cross-table transactions. |
| `crates/agent/src/db.rs::{DbThread, load_thread, save_thread, delete_thread}` | `acp_thread/snapshot_store.rs`, `crates/db/src/acp/acp_thread_snapshots/` | ACP encodes and validates versioned `SessionSnapshot` JSON; `aow-db` stores and retrieves opaque bytes in `acp_thread_snapshots`. |
| `crates/agent/src/agent.rs::{enqueue_save, run_save_worker, flush_threads_on_quit}` | Metadata operation worker; `AgentServerStore::flush_threads_on_quit` | Coalesced pending state and explicit shutdown drain. AoW captures live state after coalescing and checkpoints streaming content too; Zed's native-agent serializer excludes its incomplete streaming message. |

The existing `acp_thread/history.rs::ThreadStore` is host glue for active snapshots
and lazy body loading. Archive queries use the metadata cache and do not hydrate
all transcripts on startup. Loading a damaged body reports an error for that
conversation without making the entire history list unreadable.

`aow-acp` depends on `aow-db`, which has no dependency on ACP session models or
agent providers. The database crate exposes the `AcpDb` data source and storage
records; connections and table operations remain private. Each writer batch
commits both tables in one transaction. Caching, queue coalescing, retries and
snapshot encoding stay in ACP. Table SQL remains in separate files loaded with
`include_str!`. This extraction changes neither database paths nor schema versions.

AoW's facade keeps string thread IDs, one cwd per conversation and JSON-encoded
`folder_paths`. `entry_by_session` scopes remote IDs by agent and cwd. The reserved
upstream worktree/remote columns remain available but their editor-specific
features are not implemented. The database is not binary/schema-version compatible
with a Zed installation; do not point it at Zed's database file.

The facade and HTTP fields now use the SQL names `thread_id` and `session_id`.
`session_id` is nullable until the Agent creates its session. The host preparation
boundary in `agent_servers/lifecycle.rs` commits a thread and empty snapshot before
connecting or initializing an adapter. The input owner then creates/loads/resumes
it; retries preserve the thread ID. This requires no SQL schema migration.
Snapshots use these field names without aliases. `session_info` resolves a
thread's cwd from metadata without loading
its cached body; the tab URL no longer carries workspace identity.

## Writes and recovery

- Sidebar ordering and displayed time follow Zed's
  `sidebar::Sidebar::thread_display_time`: use `interacted_at`, falling back to
  `updated_at` for older records. AoW now carries the existing SQLite interaction
  column through session snapshots, metadata, and both desktop/mobile lists.
  Creation seeds it once; import uses the provider's historical timestamp.
  Only an accepted user prompt advances it, in the same durable write as the
  prompt. Failed persistence restores the previous value. Loading, replay,
  connection/configuration changes, streamed replies and completion preserve it.
  Legacy hydration freezes the stored update time before recovery changes it.
  `updated_at` and revision still track snapshot changes for synchronization.
- SQLite uses WAL, foreign keys and `synchronous=FULL`. The database is created
  with mode `0600`; SQLite sidecars inherit its private permissions.
- A dedicated writer replaces GPUI's background executor. The queue holds a
  reference to the latest live session, rather than cloning its body on each
  notification, and coalesces streaming bursts on a fixed one-second deadline.
  The writer captures a snapshot under the session's lock before taking the
  SQLite lock. Serialization and commits happen after releasing the session lock.
  Metadata is derived from that same captured revision so both records agree.
- User submission, thread creation/import, permission transitions, turn completion,
  disconnect and deletion wait for queued operations to commit. Metadata and its
  cached body share a transaction, including deletion.
- A failed transaction leaves both tables unchanged, reports the failure at flush
  boundaries, and retains the latest pending state for retry. Older failed writes
  cannot supersede newer queued updates or deletes. If saving a user submission
  fails, it is not sent to the agent; the unsent entry/working state are restored
  so the same conversation can retry.
- The server explicitly calls the facade's shutdown flush after draining HTTP and
  local CLI requests. Dropping a store also drains its writer. An abrupt process
  or machine failure can lose streaming chunks since the last successful commit.
- On restart, sessions are disconnected and pending permissions are cleared.
  Active prompts are never restored or resent. The agent remains authoritative
  when a session is explicitly loaded/resumed.

## Host storage paths

The active layout is `<state directory>/acp` and
`<configuration directory>/acp/settings.json`. Configuration is prepared at this
path before upgrading. The service opens only the active state directory;
former directories are left untouched and do not trigger migration, conflict
checks or installation-receipt rewrites. An empty ACP directory starts with empty
local history and requires Registry adapters to be installed again.

## Snapshot storage

Only SQLite stores local history. Existing `history/*.json` files are ignored and
left untouched, including malformed files. Cached transcripts retain their stored
entry structure; loading does not convert old activity or compaction entries.
Consumed legacy terminal output lives in `terminals` only. Loading an old cache
removes duplicate tool metadata without appending it again; if the canonical
terminal output is missing, the retained legacy output is recovered first.
The normalized body is persisted on its next save.
Corrupt databases and newer schema versions report errors instead of silently
resetting history or falling back to JSON files. SQLite schema initialization and
version management remain part of the storage boundary.

## Browser synchronization

Browser snapshot/action reads request `view=overview`. They contain session state
and turn overviews (prompts, conclusions, outcomes and process counts), with no
process bodies or terminal output. Subsequent `/sessions/{thread_id}/updates`
reads use generation/revision cursors and the same view. Only changed turns are
sent; unchanged user text and images keep their previous browser references.
Preparation and in-flight full responses cannot select a different polling mode:
a browser without an overview fetches one, including at the same revision.

Expanded processes read `/sessions/{thread_id}/process?turn_id=...`. The backend
uses a live turn-range index and clones only that turn and its referenced terminals.
A bounded journal retains positions and UTF-8 offsets so subsequent detail reads
send changed entries and text/terminal appends. Overviews are cached per turn and
ordinary stream notifications invalidate only the affected turn. These indexes
and projections are excluded from persisted canonical snapshots.

Unchanged cursors return no transcript data. Expired cursors, server restarts and
history replacement resynchronize within the requested overview/detail scope.
Visibility changes rebuild the turn index when needed. If turn boundaries or user
blocks change, both overview and process cursors resynchronize within their scope;
clients cannot retain removed turns or reuse obsolete detail positions.
Collapsing a process unmounts its components, aborts in-flight requests and drops
the detail data and cursor; reopening fetches fresh details. No closed-process
cache is retained in this first phase. The SQLite body remains authoritative and
the full-snapshot facade remains available to non-browser callers.

Polling and lazy process loading are AoW host adaptations. Zed subscribes to GPUI
entry events and uses a virtual list. Historical list virtualization and incremental
database tables are deferred until the folded-view performance is measured.

## Regression coverage

The metadata-store tests cover the upstream operation-deduplication cases, lazy
archive reads, transcript/state round trips, private file permissions, ignored
JSON files, schema rejection, failed-write rollback/retry, stream
coalescing, periodic checkpoints, deletion ordering and drop-time flushing.
The stdio integration suite verifies unchanged ACP behavior and explicit shutdown
flushing while a prompt is still streaming. Frontend history/import behavior
uses the renamed HTTP identity fields. Additional tests cover pre-connection
durability, failed initialization/creation and retry, metadata-only link lookup
and interleaved shared-connection log attribution.
The `aow-db` tests cover isolated in-memory handles, byte-preserving snapshot
storage, whole-batch rollback on failed snapshot writes and metadata deletes,
retry and reopening. ACP fault-injection tests use independent SQLite connections
without exposing the data source's connection. A fixed pre-origin schema fixture
checks upgrades independently of the current migration implementation.
