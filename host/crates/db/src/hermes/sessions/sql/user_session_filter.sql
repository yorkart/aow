s.source IN ('cli', 'tui') {delegate} AND (s.parent_session_id IS NULL OR {branch}
EXISTS(SELECT 1 FROM sessions p WHERE p.id = s.parent_session_id
    AND p.end_reason = 'branched' AND s.started_at >= p.ended_at) {compression})
