INSERT INTO threads (
    id, rollout_path, cwd, title, created_at, updated_at, updated_at_ms,
    archived, first_user_message, thread_source, source, agent_nickname, agent_role
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
