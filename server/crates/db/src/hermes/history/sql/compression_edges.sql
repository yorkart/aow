WITH RECURSIVE compression_edges(parent_id, child_id) AS (
    SELECT parent.id, (
        SELECT child.id FROM sessions child WHERE child.parent_session_id = parent.id
            AND child.source IN ('cli', 'tui') {origin}
        ORDER BY CASE WHEN child.end_reason = 'compression' THEN 0
            WHEN child.ended_at IS NULL THEN 1 ELSE 2 END,
            COALESCE((SELECT MAX(timestamp) FROM messages WHERE session_id = child.id), child.started_at) DESC,
            child.started_at DESC, child.id DESC LIMIT 1
    ) FROM sessions parent WHERE parent.end_reason = 'compression'
)
