SELECT id, rollout_path, cwd, {title}, created_at, updated_at
FROM threads
WHERE archived = 0
  AND first_user_message <> ''
  AND cwd = ?1
  AND (thread_source IS NULL OR lower(thread_source) <> 'subagent')
  AND COALESCE(source, '') NOT LIKE '{"subagent"%'
  AND COALESCE(agent_nickname, '') = ''
  AND COALESCE(agent_role, '') = ''
ORDER BY updated_at_ms DESC, id DESC
