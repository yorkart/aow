CREATE TABLE save_counts(id INTEGER);
CREATE TRIGGER count_snapshots AFTER UPDATE ON acp_thread_snapshots BEGIN INSERT INTO save_counts VALUES (1); END;
