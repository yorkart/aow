CREATE TRIGGER reject_workspace_delete BEFORE DELETE ON sidebar_threads
BEGIN
    SELECT RAISE(ABORT, 'workspace delete failed');
END;
