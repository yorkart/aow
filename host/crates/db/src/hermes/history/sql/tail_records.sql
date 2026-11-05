{history} SELECT id, session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason
FROM messages WHERE session_id IN (SELECT id FROM history) AND id > ?2 {active} ORDER BY id LIMIT ?3
