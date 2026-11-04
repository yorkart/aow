CREATE TABLE threads (
    id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, title TEXT,
    created_at INTEGER, updated_at INTEGER, updated_at_ms INTEGER,
    archived INTEGER, first_user_message TEXT, thread_source TEXT,
    source TEXT, agent_nickname TEXT, agent_role TEXT
);
