{} SELECT EXISTS(SELECT 1 FROM messages WHERE session_id IN (SELECT id FROM history) AND compacted = 1)
