CREATE TABLE acp_thread_snapshots(
    thread_id TEXT PRIMARY KEY NOT NULL REFERENCES sidebar_threads(thread_id) ON DELETE CASCADE,
    data_type TEXT NOT NULL,
    data BLOB NOT NULL
) STRICT;
