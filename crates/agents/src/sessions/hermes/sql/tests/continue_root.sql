UPDATE sessions SET parent_session_id = 'root' WHERE id IN ('continuation', 'branch')
