use super::*;

use super::history::compression_edges;

pub(super) fn query_sessions(
    home: &Path,
    id: Option<&str>,
    cwd: Option<&Path>,
) -> rusqlite::Result<Vec<AgentSession>> {
    let connection = open_db(&home.join("state.db"))?;
    let archived = if has_column(&connection, "sessions", "archived") {
        "AND s.archived = 0"
    } else {
        ""
    };
    let visible = user_session_filter(&connection, id.is_some());
    let edges = compression_edges(&connection);
    let active = if has_column(&connection, "messages", "compacted") {
        "AND (m.active = 1 OR m.compacted = 1)"
    } else if has_column(&connection, "messages", "active") {
        "AND m.active = 1"
    } else {
        ""
    };
    let sql = format!(
        r#"
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
    "#
    );
    let mut statement = connection.prepare(&sql)?;
    statement
        .query_map(
            params![
                id,
                cwd.map(|path| path.to_string_lossy().into_owned()),
                cwd.map(|path| canonical_cwd(path).to_string_lossy().into_owned())
            ],
            |row| {
                let session_id: String = row.get(0)?;
                let title: Option<String> = row.get(2)?;
                let first_user: Option<String> = row.get(5)?;
                Ok(session(
                    AgentSessionLocator {
                        agent: "hermes",
                        title: title
                            .as_deref()
                            .and_then(normalize_title)
                            .or_else(|| {
                                first_user
                                    .as_deref()
                                    .and_then(content_text)
                                    .as_deref()
                                    .and_then(normalize_title)
                            })
                            .unwrap_or_else(|| fallback_title("Hermes", &session_id)),
                        session_id,
                        cwd: PathBuf::from(row.get::<_, String>(1)?),
                        transcript_path: home.join("state.db"),
                        trusted_root: home.to_path_buf(),
                    },
                    timestamp(row.get(3)?),
                    timestamp(row.get(4)?),
                ))
            },
        )?
        .collect()
}

pub(in crate::sessions) fn canonical_cwd(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
}

fn user_session_filter(connection: &Connection, include_continuations: bool) -> String {
    let (branch, delegate) = if has_column(connection, "sessions", "model_config") {
        let config = "CASE WHEN json_valid(s.model_config) THEN s.model_config ELSE '{}' END";
        (
            format!("json_extract({config}, '$._branched_from') IS NOT NULL OR "),
            format!("AND json_extract({config}, '$._delegate_from') IS NULL"),
        )
    } else {
        (String::new(), String::new())
    };
    let compression = if include_continuations {
        "OR EXISTS(SELECT 1 FROM sessions p WHERE p.id = s.parent_session_id AND p.end_reason = 'compression')"
    } else {
        ""
    };
    format!(
        "s.source IN ('cli', 'tui') {delegate} AND (s.parent_session_id IS NULL OR {branch}
        EXISTS(SELECT 1 FROM sessions p WHERE p.id = s.parent_session_id
            AND p.end_reason = 'branched' AND s.started_at >= p.ended_at) {compression})"
    )
}
