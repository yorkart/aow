UPDATE messages SET active = 0, compacted = 1 WHERE session_id = ?1 AND active = 1
