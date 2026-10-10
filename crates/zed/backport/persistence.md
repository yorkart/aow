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
`<state directory>/zed/threads.sqlite`. A cached body is a versioned JSON blob
inside SQLite, following the serialization boundary in `agent::db`; it is not a
standalone JSON file or a log of every protocol notification. Imported sessions
have only metadata and an empty `needs_load` placeholder until explicitly opened.

## Code correspondence

Upstream paths are relative to the Zed repository; local paths are relative to
`crates/zed/src` unless indicated otherwise.

| Upstream | Local counterpart | Preserved shape / host adaptation |
| --- | --- | --- |
| `crates/agent_ui/src/thread_metadata_store.rs::ThreadMetadata`, `ThreadMetadataStore` | `agent_ui/thread_metadata_store/mod.rs` | Metadata cache, `entry`, `entries`, `entry_by_session`, `save`, `save_all`, `save_internal`, `reload`, `delete`. GPUI subscriptions become calls from the existing ACP reducer. |
| `ThreadMetadataDb::{MIGRATIONS, LIST_QUERY, list, save, delete}` | `agent_ui/thread_metadata_store/db.rs` | `sidebar_threads` and upstream column/SQL operation names. Start from the pinned schema, with new append-only migrations for later changes. |
| `DbOperation`, `dedup_db_operations`, background operation drain | `agent_ui/thread_metadata_store/operations.rs` and `mod.rs` | Latest upsert/delete per thread wins; the single writer serializes persistence. AoW adds the cached body to the operation so both records commit atomically. |
| `crates/db`, `sqlez::Connection::migrate` | `db/mod.rs` | Thin `rusqlite` connection and domain/version migration adapter; no GPUI globals or editor workspace database dependency. |
| `crates/agent/src/db.rs::{DbThread, load_thread, save_thread, delete_thread}` | `acp_thread/snapshot_store.rs` | Versioned serialized-body boundary; AoW's `SessionSnapshot` in its own `acp_thread_snapshots` table, using uncompressed JSON rather than native-agent Zstd data. |
| `crates/agent/src/agent.rs::{enqueue_save, run_save_worker, flush_threads_on_quit}` | Metadata operation worker; `AgentServerStore::flush_threads_on_quit` | Coalesced pending state and explicit shutdown drain. AoW checkpoints streaming content too; Zed's native-agent serializer excludes its incomplete streaming message. |
| AoW legacy snapshots | `agent_ui/thread_metadata_store/migration.rs` | One-time transactional import and completion marker; never part of upstream migrations. |

The existing `acp_thread/history.rs::ThreadStore` is host glue for active snapshots
and lazy body loading. Archive queries use the metadata cache and do not hydrate
all transcripts on startup. Loading a damaged body reports an error for that
conversation without making the entire history list unreadable.

AoW's facade keeps string thread IDs, one cwd per conversation and JSON-encoded
`folder_paths`. `entry_by_session` scopes remote IDs by agent and cwd. The reserved
upstream worktree/remote columns remain available but their editor-specific
features are not implemented. The database is not binary/schema-version compatible
with a Zed installation; do not point it at Zed's database file.

## Writes and recovery

- SQLite uses WAL, foreign keys and `synchronous=FULL`. The database is created
  with mode `0600`; SQLite sidecars inherit its private permissions.
- A dedicated writer replaces GPUI's background executor. The queue holds the
  latest operation per thread and coalesces streaming bursts on a fixed 100 ms
  deadline. Snapshot serialization and commits happen on that writer.
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

## Legacy JSON migration

On the first SQLite startup, `history/*.json` is imported in a single transaction
with the `AowJsonHistory` completion marker. IDs, titles, timestamps, revisions,
content and `needs_load` are preserved; old activity-layout upgrades still apply.
Existing database rows are not overwritten. Malformed JSON aborts the whole
migration with its source path, leaving the source files intact for repair/retry.

After a successful migration, old JSON files remain unchanged as backups. They
are no longer read or updated, so restarting cannot resurrect deleted sessions
or replace newer SQLite content with old snapshots. This is a one-way migration.
Corrupt databases and newer schema versions report errors instead of silently
resetting history or falling back to the old files.

## Regression coverage

The metadata-store tests cover the upstream operation-deduplication cases, lazy
archive reads, transcript/state round trips, private file permissions, atomic and
idempotent migration, schema rejection, failed-write rollback/retry, stream
coalescing, periodic checkpoints, deletion ordering and drop-time flushing.
The stdio integration suite verifies unchanged ACP behavior and explicit shutdown
flushing while a prompt is still streaming. Frontend history/import behavior
continues to use the unchanged HTTP snapshot facade.
