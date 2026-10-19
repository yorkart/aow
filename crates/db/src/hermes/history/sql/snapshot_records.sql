{history}
SELECT id, session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason
FROM messages WHERE session_id IN (SELECT id FROM history) {active}
AND id >= COALESCE((SELECT id FROM messages
    WHERE session_id IN (SELECT id FROM history) AND role = 'user' {active}
    ORDER BY id DESC LIMIT 1 OFFSET ?2), 0)
ORDER BY id
