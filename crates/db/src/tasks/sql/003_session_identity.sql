ALTER TABLE tasks RENAME COLUMN agent_id TO agent_runtime_id;
ALTER TABLE tasks ADD COLUMN agent_role_id TEXT;
ALTER TABLE tasks ADD COLUMN session_id TEXT;
CREATE INDEX tasks_session ON tasks(session_id) WHERE session_id IS NOT NULL;
