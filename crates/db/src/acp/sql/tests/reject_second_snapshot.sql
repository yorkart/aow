CREATE TRIGGER reject_snapshot BEFORE INSERT ON acp_thread_snapshots
WHEN NEW.thread_id = 'b'
BEGIN SELECT RAISE(ABORT, 'test write failure'); END;
