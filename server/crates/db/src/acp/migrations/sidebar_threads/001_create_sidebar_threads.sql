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
