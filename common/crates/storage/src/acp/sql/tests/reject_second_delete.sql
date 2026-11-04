CREATE TRIGGER reject_delete BEFORE DELETE ON sidebar_threads
WHEN OLD.thread_id = 'b'
BEGIN SELECT RAISE(ABORT, 'test delete failure'); END;
