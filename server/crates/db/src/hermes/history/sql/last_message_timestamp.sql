{history} SELECT MAX(timestamp) FROM messages WHERE session_id IN (SELECT id FROM history)
