use super::*;
use aow_db::hermes::HermesDb;

pub(super) fn query_sessions(
    home: &Path,
    id: Option<&str>,
    cwd: Option<&Path>,
) -> anyhow::Result<Vec<AgentSession>> {
    let db = HermesDb::open(&home.join("state.db"))?;
    let original_cwd = cwd.map(|path| path.to_string_lossy());
    let resolved_cwd = cwd.map(|path| canonical_cwd(path).to_string_lossy().into_owned());
    Ok(db
        .query_sessions(id, original_cwd.as_deref(), resolved_cwd.as_deref())?
        .into_iter()
        .map(|row| {
            let title = row
                .title
                .as_deref()
                .and_then(normalize_title)
                .or_else(|| {
                    row.first_user_message
                        .as_deref()
                        .and_then(content_text)
                        .as_deref()
                        .and_then(normalize_title)
                })
                .unwrap_or_else(|| fallback_title("Hermes", &row.id));
            session(
                AgentSessionLocator {
                    agent: "hermes",
                    title,
                    session_id: row.id,
                    cwd: PathBuf::from(row.cwd),
                    transcript_path: home.join("state.db"),
                    trusted_root: home.to_path_buf(),
                },
                timestamp(row.started_at),
                timestamp(row.updated_at),
            )
        })
        .collect())
}

pub(in crate::sessions) fn canonical_cwd(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize_path(path))
}
