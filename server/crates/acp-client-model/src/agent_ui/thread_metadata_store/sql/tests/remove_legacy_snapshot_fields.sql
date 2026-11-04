UPDATE acp_thread_snapshots SET data = CAST(json_remove(CAST(data AS TEXT), '$.source', '$.created_at', '$.interacted_at') AS BLOB) WHERE thread_id = 'old'
