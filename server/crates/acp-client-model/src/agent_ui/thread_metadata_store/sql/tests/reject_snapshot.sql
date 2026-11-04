CREATE TRIGGER reject_snapshot BEFORE INSERT ON acp_thread_snapshots BEGIN SELECT RAISE(ABORT, 'test write failure'); END;
