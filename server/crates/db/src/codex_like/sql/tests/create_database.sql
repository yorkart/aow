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

INSERT INTO threads (
    id, rollout_path, cwd, title, created_at, updated_at, updated_at_ms,
    archived, first_user_message
) VALUES
    ('first', '/profile/sessions/first.jsonl', '/repo', 'First prompt', 1725100000, 1725100123000, 1725100123000, 0, 'prompt'),
    ('second', '/profile/sessions/second.jsonl', '/repo', 'Second prompt', 1725100000, 1725100200, 1725100200000, 0, 'prompt');
