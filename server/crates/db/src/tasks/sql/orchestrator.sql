-- Preserve task IDs, conversation ownership and notification references.
ALTER TABLE tasks RENAME COLUMN is_router TO is_orchestrator;
DROP INDEX tasks_router_project;
CREATE UNIQUE INDEX tasks_orchestrator_project ON tasks(project_id) WHERE is_orchestrator = 1;
UPDATE tasks SET prompt = 'Orchestrator' WHERE is_orchestrator = 1 AND prompt = 'Router';
