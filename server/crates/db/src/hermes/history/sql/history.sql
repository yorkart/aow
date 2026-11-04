{edges}, history(id) AS (
    SELECT id FROM sessions WHERE id = ?1
    UNION
    SELECT CASE WHEN e.parent_id = h.id THEN e.child_id ELSE e.parent_id END
    FROM history h JOIN compression_edges e ON e.parent_id = h.id OR e.child_id = h.id
    WHERE e.child_id IS NOT NULL
)
