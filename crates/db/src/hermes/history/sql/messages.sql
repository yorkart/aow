{} SELECT id, session_id, role, content, tool_call_id, tool_calls,
timestamp, finish_reason, active, compacted FROM messages
WHERE session_id IN (SELECT id FROM history) ORDER BY id
