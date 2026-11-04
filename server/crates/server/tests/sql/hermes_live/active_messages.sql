SELECT id, role, content, tool_call_id, tool_calls, timestamp, finish_reason, reasoning FROM messages WHERE session_id = ?1 AND active = 1 ORDER BY id
