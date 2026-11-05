-- Replace the constrained column in place so task relationships and resources survive.
ALTER TABLE tasks ADD COLUMN next_status TEXT NOT NULL DEFAULT 'todo'
    CHECK (next_status IN ('backlog', 'todo', 'in_progress', 'in_review', 'done', 'cancelled', 'blocked'));
UPDATE tasks SET next_status = CASE status WHEN 'waiting' THEN 'blocked' ELSE status END;
ALTER TABLE tasks DROP COLUMN status;
ALTER TABLE tasks RENAME COLUMN next_status TO status;

-- Historical tasks have no recorded timestamps; leave those unknown values NULL.
ALTER TABLE tasks ADD COLUMN created_at INTEGER;
ALTER TABLE tasks ADD COLUMN updated_at INTEGER;
ALTER TABLE tasks ADD COLUMN completed_at INTEGER;
