ALTER TABLE tasks ADD COLUMN status TEXT NOT NULL DEFAULT 'todo'
    CHECK (status IN ('todo', 'in_progress', 'waiting', 'in_review', 'done', 'blocked', 'cancelled'));
CREATE INDEX task_runs_thread ON task_runs(thread_id);

-- Execution context belongs to each run; the task fields remain its defaults.
UPDATE task_runs SET payload = json_set(payload,
    '$.trigger', CASE WHEN request_id = 'initial' THEN 'assignment' ELSE 'message' END,
    '$.execution', (SELECT json_object(
        'agent_id', agent_id, 'agent_role_id', agent_role_id,
        'workspace', workspace, 'workspace_intent', workspace_intent,
        'target_task_id', target_task_id, 'target_revision', target_revision
    ) FROM tasks WHERE tasks.id = task_runs.task_id));
UPDATE tasks SET status = 'in_progress' WHERE EXISTS (
    SELECT 1 FROM task_runs WHERE task_id = tasks.id
);
UPDATE tasks SET status = 'waiting' WHERE EXISTS (
    SELECT 1 FROM task_runs WHERE task_id = tasks.id AND state = 'waiting'
);
