ALTER TABLE tasks ADD COLUMN is_router INTEGER NOT NULL DEFAULT 0 CHECK(is_router IN (0, 1));
ALTER TABLE tasks ADD COLUMN source_task_id TEXT;
ALTER TABLE tasks ADD COLUMN source_thread_id TEXT;
ALTER TABLE tasks ADD COLUMN workspace_intent TEXT;
ALTER TABLE tasks ADD COLUMN target_task_id TEXT;
ALTER TABLE tasks ADD COLUMN target_revision TEXT;
CREATE UNIQUE INDEX tasks_router_project ON tasks(project_id) WHERE is_router = 1;
CREATE INDEX tasks_source ON tasks(source_task_id);
