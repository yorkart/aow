PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;
CREATE TABLE IF NOT EXISTS schema_migrations(domain TEXT PRIMARY KEY NOT NULL, version INTEGER NOT NULL) STRICT;
CREATE TABLE sidebar_threads(
    thread_id TEXT PRIMARY KEY NOT NULL,
    session_id TEXT,
    agent_id TEXT,
    title TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    created_at TEXT,
    interacted_at TEXT,
    folder_paths TEXT,
    folder_paths_order TEXT,
    archived INTEGER DEFAULT 0,
    main_worktree_paths TEXT,
    main_worktree_paths_order TEXT,
    remote_connection TEXT,
    title_override TEXT
) STRICT;
CREATE INDEX sidebar_threads_updated_at ON sidebar_threads(updated_at DESC);
ALTER TABLE sidebar_threads ADD COLUMN parent TEXT;
CREATE TABLE acp_thread_snapshots(
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES sidebar_threads(thread_id) ON DELETE CASCADE,
    data_type TEXT NOT NULL,
    data BLOB NOT NULL
) STRICT;
INSERT INTO schema_migrations(domain, version) VALUES ('ThreadMetadataDb', 2), ('AowAcpSnapshotDb', 1);
