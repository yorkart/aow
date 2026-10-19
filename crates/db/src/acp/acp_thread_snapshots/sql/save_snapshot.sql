INSERT INTO acp_thread_snapshots(thread_id, data_type, data) VALUES (?1, 'json', ?2)
ON CONFLICT(thread_id) DO UPDATE SET data_type = excluded.data_type, data = excluded.data
