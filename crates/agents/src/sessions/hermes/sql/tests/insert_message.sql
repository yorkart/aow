INSERT INTO messages (session_id, role, content, timestamp, finish_reason, tool_calls, tool_call_id, reasoning)
VALUES (?1, ?2, ?3, COALESCE((SELECT MAX(timestamp) + 0.01 FROM messages), 1700000001.5), ?4, ?5, ?6, 'PRIVATE_REASONING')
