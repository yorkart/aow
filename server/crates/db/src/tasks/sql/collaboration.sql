-- Execution records and receipts outlive task organization. Do not cascade on
-- task deletion: undeliverable results must remain available in the Inbox.
CREATE TABLE task_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    request_id TEXT NOT NULL,
    thread_id TEXT NOT NULL,
    parent_run_id TEXT,
    state TEXT NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(task_id, request_id)
);
CREATE INDEX task_runs_task ON task_runs(task_id);
CREATE INDEX task_runs_state ON task_runs(state);
CREATE INDEX task_runs_parent ON task_runs(parent_run_id);
CREATE TABLE task_inbox (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL UNIQUE REFERENCES task_runs(id),
    project_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    is_read INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX task_inbox_order ON task_inbox(created_at DESC, id);
CREATE TABLE task_wakeups (
    id TEXT PRIMARY KEY REFERENCES task_inbox(id),
    prompt_id TEXT NOT NULL UNIQUE,
    continuation_run_id TEXT,
    state TEXT NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX task_wakeups_state ON task_wakeups(state);
CREATE INDEX task_wakeups_continuation ON task_wakeups(continuation_run_id);
