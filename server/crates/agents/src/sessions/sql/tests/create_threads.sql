CREATE TABLE threads (
    id TEXT PRIMARY KEY,
    rollout_path TEXT NOT NULL,
    cwd TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    archived INTEGER NOT NULL,
    first_user_message TEXT NOT NULL,
    thread_source TEXT,
    source TEXT,
    agent_nickname TEXT,
    agent_role TEXT
);
