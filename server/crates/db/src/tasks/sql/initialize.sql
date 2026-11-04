CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    prompt TEXT NOT NULL CHECK(length(trim(prompt)) > 0),
    agent_id TEXT NOT NULL,
    workspace TEXT NOT NULL,
    workspace_mode TEXT NOT NULL CHECK(workspace_mode IN ('existing', 'new_worktree')),
    slug TEXT,
    initial_thread_id TEXT,
    pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0, 1)),
    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0, 1))
);
CREATE TABLE task_dependencies (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    dependency_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    PRIMARY KEY(task_id, dependency_id),
    CHECK(task_id != dependency_id)
);
CREATE TABLE task_resources (
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('acp', 'terminal')),
    resource_id TEXT NOT NULL,
    PRIMARY KEY(kind, resource_id)
);
CREATE INDEX tasks_project ON tasks(project_id);
CREATE INDEX task_resources_owner ON task_resources(task_id);
