{edges}, lineage(root_id, id) AS (
    SELECT s.id, s.id FROM sessions s
    WHERE (?1 IS NULL OR s.id = ?1) AND (?2 IS NULL OR s.cwd = ?2 OR s.cwd = ?3)
      AND {visible} AND s.cwd IS NOT NULL
    UNION
    SELECT h.root_id, e.child_id FROM lineage h
    JOIN compression_edges e ON e.parent_id = h.id WHERE e.child_id IS NOT NULL
)
SELECT s.id, s.cwd,
    COALESCE((SELECT tip.title FROM lineage h JOIN sessions tip ON tip.id = h.id
        WHERE h.root_id = s.id AND NULLIF(TRIM(tip.title), '') IS NOT NULL
          AND NOT EXISTS(SELECT 1 FROM compression_edges e WHERE e.parent_id = tip.id AND e.child_id IS NOT NULL)
        LIMIT 1), s.title),
    s.started_at,
    COALESCE((SELECT MAX(m.timestamp) FROM messages m JOIN lineage h ON h.id = m.session_id
        WHERE h.root_id = s.id {active}), s.ended_at, s.started_at),
    (SELECT m.content FROM messages m JOIN lineage h ON h.id = m.session_id
        WHERE h.root_id = s.id AND m.role = 'user' {active} ORDER BY m.id LIMIT 1)
FROM sessions s WHERE s.id IN (SELECT root_id FROM lineage)
  AND (?1 IS NOT NULL OR (
    EXISTS(SELECT 1 FROM messages m JOIN lineage h ON h.id = m.session_id
        WHERE h.root_id = s.id AND m.role = 'user' {active})
    {archived}
  ))
ORDER BY 5 DESC, s.id DESC
