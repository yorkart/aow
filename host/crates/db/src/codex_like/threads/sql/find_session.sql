SELECT id, rollout_path, cwd, {title}, created_at, updated_at
FROM threads
WHERE id = ?1
