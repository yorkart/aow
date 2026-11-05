OR EXISTS(SELECT 1 FROM sessions p WHERE p.id = s.parent_session_id AND p.end_reason = 'compression')
