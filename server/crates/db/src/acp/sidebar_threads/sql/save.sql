INSERT INTO sidebar_threads(thread_id, session_id, agent_id, title, updated_at,
created_at, interacted_at, folder_paths, archived, title_override, parent, source)
VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
ON CONFLICT(thread_id) DO UPDATE SET
    session_id = excluded.session_id,
    agent_id = excluded.agent_id,
    title = excluded.title,
    updated_at = excluded.updated_at,
    created_at = excluded.created_at,
    interacted_at = excluded.interacted_at,
    folder_paths = excluded.folder_paths,
    archived = excluded.archived,
    title_override = excluded.title_override,
    parent = excluded.parent,
    source = excluded.source
