//! Pi's native, append-only session trees.

#[cfg(test)]
mod tests;
mod transcript;

use super::*;
pub(crate) use transcript::{Transcript, text_content};

pub(super) struct Pi;

impl AgentSessionProvider for Pi {
    fn session_root(&self, process_home: &Path, environment: &SessionEnvironment) -> PathBuf {
        if let Some(binding) = binding(environment)
            && let Some(file) = binding["session_file"].as_str()
            && let Some(parent) = Path::new(file).parent()
        {
            return parent.to_path_buf();
        }
        let expand = |path: &Path| {
            path.strip_prefix("~")
                .map_or_else(|_| path.to_path_buf(), |rest| process_home.join(rest))
        };
        if let Some(root) = environment.get("PI_CODING_AGENT_SESSION_DIR") {
            return expand(root);
        }
        let directory = environment
            .get("PI_CODING_AGENT_DIR")
            .map(|path| expand(path))
            .unwrap_or_else(|| process_home.join(".pi/agent"));
        let settings = std::fs::read(directory.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        settings
            .as_ref()
            .and_then(|settings| settings["sessionDir"].as_str())
            .filter(|path| !path.is_empty())
            .map(|path| expand(Path::new(path)))
            .unwrap_or_else(|| directory.join("sessions"))
    }

    fn list_sessions(&self, roots: &SessionRoots, workspace_path: &Path) -> Vec<AgentSession> {
        scan(&roots.pi)
            .filter_map(|path| {
                let transcript = Transcript::read(&path).ok()?;
                path_is_inside_or_equal(&transcript.cwd, workspace_path)
                    .then(|| transcript.session(&path, &roots.pi))
                    .flatten()
            })
            .collect()
    }

    fn find_session(&self, roots: &SessionRoots, session_id: &str) -> Option<AgentSession> {
        // Full IDs only. An ambiguous duplicate must never resume a different file.
        let mut sessions = scan(&roots.pi).filter_map(|path| {
            let transcript = Transcript::read(&path).ok()?;
            (transcript.id == session_id)
                .then(|| transcript.session(&path, &roots.pi))
                .flatten()
        });
        let session = sessions.next()?;
        sessions.next().is_none().then_some(session)
    }

    fn current_title(&self, locator: &AgentSessionLocator) -> Option<String> {
        let transcript = Transcript::read(&locator.transcript_path).ok()?;
        (transcript.id == locator.session_id).then(|| transcript.title())
    }

    fn read_snapshot(
        &self,
        locator: AgentSessionLocator,
    ) -> Result<snapshot::AgentSessionSnapshot, snapshot::SnapshotError> {
        snapshot::read_pi(locator)
    }
}

pub(crate) fn binding(environment: &SessionEnvironment) -> Option<Value> {
    let path = environment.get("AOW_PI_BINDING")?;
    // Bindings contain only identity metadata, never conversation text.
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > 16 * 1024 {
        return None;
    }
    serde_json::from_reader(file).ok()
}

fn scan(root: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| {
            !root.as_os_str().is_empty()
                && entry.file_type().is_file()
                && entry.path().extension() == Some(OsStr::new("jsonl"))
        })
        .map(|entry| entry.into_path())
}
