CREATE TRIGGER reject_writes BEFORE INSERT ON acp_thread_snapshots BEGIN SELECT RAISE(ABORT, 'test disk failure'); END;
