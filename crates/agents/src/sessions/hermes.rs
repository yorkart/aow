//! Hermes 0.18 native history. Connections are read-only, including WAL reads;
//! no Hermes Python imports, schema migrations, hooks or exports are needed.
use super::*;

mod messages;

pub(super) struct Hermes;

impl AgentSessionProvider for Hermes {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        home(process_home, environment)
    }

    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        query_sessions(&roots.hermes, None, Some(workspace_path)).unwrap_or_default()
    }

    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        query_sessions(&roots.hermes, Some(session_id), None)
            .ok()?
            .pop()
    }

    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        query_sessions(&locator.trusted_root, Some(&locator.session_id), None)
            .ok()?
            .pop()
            .map(|session| session.title)
    }

    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_hermes(locator)
    }
}

pub(super) fn home(process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
    let default = process_home.join(".hermes");
    let configured = environment
        .get("HERMES_HOME")
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(&default);
    // Hermes trusts an explicit profile, but still follows active_profile when
    // HERMES_HOME names the root (including custom roots outside ~/.hermes).
    if configured.parent().and_then(Path::file_name) == Some(OsStr::new("profiles")) {
        return configured.clone();
    }
    let root = if canonical_cwd(configured).starts_with(canonical_cwd(&default)) {
        &default
    } else {
        configured
    };
    if let Ok(profile) = std::fs::read_to_string(root.join("active_profile")) {
        let profile = profile.trim().to_ascii_lowercase();
        if profile != "default"
            && !profile.is_empty()
            && profile.len() <= 64
            && profile.as_bytes()[0].is_ascii_alphanumeric()
            && profile
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b"_-".contains(&ch))
        {
            return root.join("profiles").join(profile);
        }
    }
    configured.clone()
}

pub(super) fn open_db(path: &Path) -> rusqlite::Result<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
    Ok(connection)
}

pub(super) fn has_column(connection: &Connection, table: &str, column: &str) -> bool {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
            params![table, column],
            |row| row.get(0),
        )
        .unwrap_or(false)
}

fn query_sessions(
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

pub(super) fn canonical_cwd(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
}

pub(super) fn user_session_filter(connection: &Connection, include_continuations: bool) -> String {
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

pub(super) fn timestamp(epoch: f64) -> DateTime<Utc> {
    DateTime::from_timestamp_millis((epoch * 1000.0) as i64).unwrap_or(DateTime::UNIX_EPOCH)
}

// Match Hermes' get_compression_tip: explicit branches/delegates are separate
// conversations, and a live continuation takes precedence over stale siblings.
fn compression_edges(connection: &Connection) -> String {
    let origin = if has_column(connection, "sessions", "model_config") {
        let config =
            "CASE WHEN json_valid(child.model_config) THEN child.model_config ELSE '{}' END";
        format!(
            "AND json_extract({config}, '$._branched_from') IS NULL
                 AND json_extract({config}, '$._delegate_from') IS NULL"
        )
    } else {
        String::new()
    };
    format!(
        r#"
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
    "#
    )
}

pub(super) fn history(connection: &Connection) -> String {
    let edges = compression_edges(connection);
    format!(
        r#"{edges}, history(id) AS (
        SELECT id FROM sessions WHERE id = ?1
        UNION
        SELECT CASE WHEN e.parent_id = h.id THEN e.child_id ELSE e.parent_id END
        FROM history h JOIN compression_edges e ON e.parent_id = h.id OR e.child_id = h.id
        WHERE e.child_id IS NOT NULL
    )
    "#
    )
}

pub(super) fn snapshot_records(connection: &Connection, id: &str) -> rusqlite::Result<Vec<Value>> {
    if messages::has_compaction(connection, id)? {
        let mut records = messages::read(connection, id)?;
        // Bound public turns after removing compaction copies, not before.
        let start = records
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, record)| record["role"] == "user")
            .nth(200)
            .map_or(0, |(index, _)| index);
        records.drain(..start);
        return Ok(records);
    }
    let active = if has_column(connection, "messages", "active") {
        "AND active = 1"
    } else {
        ""
    };
    let history = history(connection);
    // Keep one extra turn so the public snapshot accurately reports truncation.
    let sql = format!(
        r#"{history}
        SELECT id, session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason
        FROM messages WHERE session_id IN (SELECT id FROM history) {active}
        AND id >= COALESCE((SELECT id FROM messages
            WHERE session_id IN (SELECT id FROM history) AND role = 'user' {active}
            ORDER BY id DESC LIMIT 1 OFFSET 200), 0)
        ORDER BY id
    "#
    );
    connection.prepare(&sql)?.query_map([id], record)?.collect()
}

pub(super) fn compacted_records(
    connection: &Connection,
    id: &str,
) -> rusqlite::Result<Option<Vec<Value>>> {
    messages::has_compaction(connection, id)?
        .then(|| messages::read(connection, id))
        .transpose()
}

pub(super) fn record(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let calls: Option<String> = row.get(5)?;
    let content: Option<String> = row.get(3)?;
    Ok(serde_json::json!({
        "id": row.get::<_, i64>(0)?, "session_id": row.get::<_, String>(1)?,
        "role": row.get::<_, String>(2)?, "content": content.as_deref().map(decode_content),
        "tool_call_id": row.get::<_, Option<String>>(4)?,
        "tool_calls": calls.map(|text| serde_json::from_str::<Value>(&text).unwrap_or(Value::String(text))),
        "timestamp": timestamp(row.get(6)?).to_rfc3339_opts(SecondsFormat::Millis, true),
        "finish_reason": row.get::<_, Option<String>>(7)?,
    }))
}

pub(super) fn terminal_reply(record: &Value) -> bool {
    record["role"] == "assistant"
        && (record["tool_calls"].is_null()
            || record["tool_calls"].as_array().is_some_and(Vec::is_empty))
        && matches!(
            record["finish_reason"].as_str(),
            Some("stop" | "end_turn" | "stop_sequence")
        )
}

pub(super) fn public_text(record: &Value) -> Option<String> {
    let mut text = text_content(&record["content"])?;
    if record["role"] == "assistant" {
        // Hermes can store thought blocks inline as well as in separate
        // reasoning columns. Those columns are never selected above.
        for tag in ["think", "thinking", "reasoning_scratchpad"] {
            let opening = format!("<{tag}>");
            let closing = format!("</{tag}>");
            loop {
                let lower = text.to_ascii_lowercase();
                let Some(start) = lower.find(&opening) else {
                    break;
                };
                let end = lower[start + opening.len()..]
                    .find(&closing)
                    .map_or(text.len(), |end| {
                        start + opening.len() + end + closing.len()
                    });
                text.replace_range(start..end, "");
            }
        }
    }
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn decode_content(content: &str) -> Value {
    // Hermes stores multimodal content with an unambiguous NUL-prefixed marker.
    // Never expose an undecodable structured payload (which can contain images).
    content.strip_prefix("\0json:").map_or_else(
        || Value::String(content.to_owned()),
        |json| serde_json::from_str(json).unwrap_or(Value::Null),
    )
}

fn content_text(content: &str) -> Option<String> {
    text_content(&decode_content(content))
}

fn text_content(content: &Value) -> Option<String> {
    match content {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(text_content)
                .collect::<Vec<_>>()
                .join("\n");
            (!text.trim().is_empty()).then_some(text)
        }
        Value::Object(part)
            if matches!(
                part.get("type").and_then(Value::as_str),
                Some("text" | "input_text" | "output_text")
            ) =>
        {
            part.get("text").and_then(Value::as_str).map(str::to_owned)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
