CREATE TABLE IF NOT EXISTS tasks (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    parent_id TEXT REFERENCES tasks(id),
    prompt TEXT NOT NULL CHECK(length(trim(prompt)) > 0),
    agent_id TEXT NOT NULL,
    workspace TEXT NOT NULL,
    thread_id TEXT NOT NULL UNIQUE,
    pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1))
);
CREATE INDEX IF NOT EXISTS tasks_parent ON tasks(parent_id);
CREATE INDEX IF NOT EXISTS tasks_project ON tasks(project_id);
