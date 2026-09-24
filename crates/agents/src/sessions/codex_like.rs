use super::*;

pub(crate) struct CodexLikeSessionProvider {
    pub(crate) definition: &'static AgentDefinition,
    pub(crate) home: PathBuf,
}

pub(super) fn current_title(
    definition: &'static AgentDefinition,
    locator: &AgentSessionLocator,
) -> Option<String> {
    // Locators retain <configured home>/sessions, even after the agent exits or
    // the server's environment changes. Never fall back to another profile.
    let session = CodexLikeSessionProvider {
        definition,
        home: locator.trusted_root.parent()?.to_path_buf(),
    }
    .find_session(&locator.session_id)?;
    (session.transcript_path == locator.transcript_path).then_some(session.title)
}

impl CodexLikeSessionProvider {
    pub(crate) fn list_sessions(&self, workspace_path: &Path) -> Vec<AgentSession> {
        let Ok(connection) = open_state_db(&self.home) else {
            return Vec::new();
        };
        let sql = r#"
SELECT id, rollout_path, cwd, title, created_at, updated_at
FROM threads
WHERE archived = 0
  AND first_user_message <> ''
  AND cwd = ?1
  AND (thread_source IS NULL OR lower(thread_source) <> 'subagent')
  AND COALESCE(source, '') NOT LIKE '{"subagent"%'
  AND COALESCE(agent_nickname, '') = ''
  AND COALESCE(agent_role, '') = ''
ORDER BY updated_at_ms DESC, id DESC
"#;
        let sql = sql.replace(
            "cwd, title,",
            &format!("cwd, {},", display_title_column(&connection)),
        );
        let Ok(mut statement) = connection.prepare(&sql) else {
            return Vec::new();
        };
        let Ok(rows) = statement
            .query_map(params![workspace_path.to_string_lossy().as_ref()], |row| {
                Self::row_session(self.definition, &self.home, row)
            })
        else {
            return Vec::new();
        };
        let mut sessions: Vec<_> = rows.filter_map(Result::ok).collect();
        if display_title_column(&connection) == "title" {
            apply_index_names(&self.home, &mut sessions);
        }
        sessions
    }

    pub(crate) fn find_session(&self, session_id: &str) -> Option<AgentSession> {
        let connection = open_state_db(&self.home).ok()?;
        let sql = r#"
SELECT id, rollout_path, cwd, title, created_at, updated_at
FROM threads
WHERE id = ?1
"#;
        let sql = sql.replace(
            "cwd, title,",
            &format!("cwd, {},", display_title_column(&connection)),
        );
        let mut session = connection
            .query_row(&sql, params![session_id], |row| {
                Self::row_session(self.definition, &self.home, row)
            })
            .optional()
            .ok()
            .flatten()?;
        if display_title_column(&connection) == "title" {
            apply_index_names(&self.home, std::slice::from_mut(&mut session));
        }
        Some(session)
    }
}

fn apply_index_names(home: &Path, sessions: &mut [AgentSession]) {
    let Ok(file) = File::open(home.join("session_index.jsonl")) else {
        return;
    };
    let positions: std::collections::HashMap<_, _> = sessions
        .iter()
        .enumerate()
        .map(|(index, session)| (session.session_id.clone(), index))
        .collect();
    // The index is append-only. Last complete entry wins; a partially written
    // trailing record must not hide the previous name.
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(index) = entry
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| positions.get(id))
        {
            if let Some(title) = entry
                .get("thread_name")
                .and_then(Value::as_str)
                .and_then(normalize_title)
            {
                sessions[*index].title = title;
            }
        }
    }
}

// Recent Codex/Trae schemas separate the user-visible name from the searchable
// first-message title. Older versions have no name column.
fn display_title_column(connection: &Connection) -> &'static str {
    let has_name = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name = 'name')",
            [],
            |row| row.get::<_, bool>(0),
        )
        .unwrap_or(false);
    if has_name {
        "COALESCE(NULLIF(name, ''), title)"
    } else {
        "title"
    }
}

impl CodexLikeSessionProvider {
    fn row_session(
        definition: &'static AgentDefinition,
        home: &Path,
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<AgentSession> {
        let session_id: String = row.get(0)?;
        let transcript_path = PathBuf::from(row.get::<_, String>(1)?);
        let cwd = PathBuf::from(row.get::<_, String>(2)?);
        let title = normalize_title(&row.get::<_, String>(3)?)
            .unwrap_or_else(|| fallback_title(definition.display_name, &session_id));
        let created_at = sqlite_timestamp(row.get(4)?);
        let updated_at = sqlite_timestamp(row.get(5)?);
        Ok(session(
            AgentSessionLocator {
                agent: definition.id,
                session_id,
                title,
                cwd,
                transcript_path,
                trusted_root: home.join("sessions"),
            },
            created_at,
            updated_at,
        ))
    }
}

fn open_state_db(home: &Path) -> rusqlite::Result<Connection> {
    let path = home.join(STATE_DB_FILENAME);
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(std::time::Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))?;
    Ok(connection)
}

pub(super) fn sqlite_timestamp(epoch: i64) -> DateTime<Utc> {
    let (seconds, nanos) = if epoch.abs() >= 100_000_000_000 {
        (
            epoch / 1_000,
            ((epoch % 1_000).unsigned_abs() * 1_000_000) as u32,
        )
    } else {
        (epoch, 0)
    };
    DateTime::<Utc>::from_timestamp(seconds, nanos).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}
