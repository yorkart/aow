SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1 AND cwd = ?2 AND ended_at IS NULL)
