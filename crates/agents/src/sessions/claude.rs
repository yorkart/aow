use super::*;

pub(super) struct Claude;

impl AgentSessionProvider for Claude {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        environment
            .get("CLAUDE_CONFIG_DIR")
            .cloned()
            .unwrap_or_else(|| process_home.join(".claude"))
    }
    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        ClaudeSessionProvider {
            root: roots.claude.clone(),
        }
        .list_sessions(workspace_path)
    }
    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        ClaudeSessionProvider {
            root: roots.claude.clone(),
        }
        .find_session(session_id)
    }
    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        parse_claude_exact(
            &locator.transcript_path,
            &locator.trusted_root,
            &locator.session_id,
        )
        .map(|session| session.title)
    }
    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_claude(locator)
    }
}

pub(crate) struct ClaudeSessionProvider {
    pub(crate) root: PathBuf,
}

impl ClaudeSessionProvider {
    pub(crate) fn list_sessions(&self, workspace_path: &Path) -> Vec<AgentSession> {
        let mut sessions = Vec::new();
        scan_claude(&self.root.join("projects"), workspace_path, &mut sessions);
        sessions
    }

    pub(crate) fn find_session(&self, session_id: &str) -> Option<AgentSession> {
        jsonl_files(&self.root.join("projects")).find_map(|path| {
            if path
                .components()
                .any(|component| component.as_os_str() == OsStr::new("subagents"))
            {
                return None;
            }
            parse_claude_exact(&path, &self.root.join("projects"), session_id)
        })
    }
}

fn scan_claude(root: &Path, worktree: &Path, sessions: &mut Vec<AgentSession>) {
    for path in jsonl_files(root) {
        if path
            .components()
            .any(|component| component.as_os_str() == OsStr::new("subagents"))
        {
            continue;
        }
        if let Some(session) = parse_claude(&path, root, worktree) {
            sessions.push(session);
        }
    }
}

fn jsonl_files(root: &Path) -> impl Iterator<Item = PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.path().extension() == Some(OsStr::new("jsonl")))
        .map(|entry| entry.into_path())
}

pub(super) fn parse_claude(
    path: &Path,
    trusted_root: &Path,
    worktree: &Path,
) -> Option<AgentSession> {
    parse_claude_with_filter(path, trusted_root, |cwd, _| {
        path_is_inside_or_equal(cwd, worktree)
    })
}

pub(super) fn parse_claude_exact(
    path: &Path,
    trusted_root: &Path,
    expected_session_id: &str,
) -> Option<AgentSession> {
    parse_claude_with_filter(path, trusted_root, |_, session_id| {
        session_id == expected_session_id
    })
}

pub(super) fn parse_claude_with_filter(
    path: &Path,
    trusted_root: &Path,
    include: impl Fn(&Path, &str) -> bool,
) -> Option<AgentSession> {
    let file = File::open(path).ok()?;
    let mut session_id = path.file_stem()?.to_string_lossy().into_owned();
    let mut cwd = None;
    let mut created_at = None;
    let mut custom_title = None;
    let mut generated_title = None;
    let mut first_user_title = None;

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(record) = record.as_object() else {
            continue;
        };
        update_created_at(&mut created_at, string(record, "timestamp"));
        if let Some(value) = string(record, "sessionId").or_else(|| string(record, "session_id")) {
            session_id = value.to_owned();
        }
        if let Some(value) = string(record, "cwd") {
            cwd = Some(PathBuf::from(value));
        }

        match string(record, "type") {
            Some("custom-title") => {
                if let Some(title) = string(record, "customTitle").and_then(normalize_title) {
                    custom_title = Some(title);
                }
            }
            Some("ai-title") => {
                if let Some(title) = string(record, "aiTitle").and_then(normalize_title) {
                    generated_title = Some(title);
                }
            }
            Some("user")
                if first_user_title.is_none()
                    && record.get("isMeta") != Some(&Value::Bool(true)) =>
            {
                let content = record
                    .get("message")
                    .and_then(Value::as_object)
                    .and_then(|message| message.get("content"))
                    .and_then(extract_text);
                if let Some(title) = content.as_deref().and_then(user_title) {
                    first_user_title = Some(title);
                }
            }
            _ => {}
        }
    }

    let cwd = cwd?;
    if !include(&cwd, &session_id) {
        return None;
    }
    let updated_at = file_updated_at(path).or(created_at)?;
    let created_at = created_at.unwrap_or(updated_at);
    let title = custom_title
        .or(generated_title)
        .or(first_user_title)
        .unwrap_or_else(|| fallback_title("Claude", &session_id));
    Some(session(
        AgentSessionLocator {
            agent: CLAUDE.id,
            session_id,
            title,
            cwd,
            transcript_path: path.to_path_buf(),
            trusted_root: trusted_root.to_path_buf(),
        },
        created_at,
        updated_at,
    ))
}

fn string<'a>(record: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    record
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn extract_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Array(items) => {
            let parts = items
                .iter()
                .filter_map(|item| {
                    let object = item.as_object()?;
                    match string(object, "type") {
                        Some("text" | "input_text" | "output_text") | None => object
                            .get("text")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned),
        _ => None,
    }
}

pub(super) fn user_title(value: &str) -> Option<String> {
    let value = value.trim();
    let injected = [
        "<local-command-caveat>",
        "<local-command-stdout>",
        "<command-name>",
        "<system-reminder>",
        "<environment_context>",
        "<permissions instructions>",
        "<collaboration_mode>",
        "<multi_agent_mode>",
        "# AGENTS.md instructions",
    ];
    if injected.iter().any(|prefix| value.starts_with(prefix)) {
        return None;
    }
    normalize_title(value)
}

fn update_created_at(target: &mut Option<DateTime<Utc>>, value: Option<&str>) {
    let Some(timestamp) = value.and_then(parse_timestamp) else {
        return;
    };
    if target.is_none_or(|current| timestamp < current) {
        *target = Some(timestamp);
    }
}

pub(super) fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn file_updated_at(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
}
