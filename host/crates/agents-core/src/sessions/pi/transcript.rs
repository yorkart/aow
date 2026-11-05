use std::collections::{HashMap, HashSet};

use super::*;
use crate::sessions::snapshot::SnapshotError;

pub(crate) struct Transcript {
    pub id: String,
    pub cwd: PathBuf,
    created_at: DateTime<Utc>,
    pub entries: Vec<Value>,
}

impl Transcript {
    pub fn read(path: &Path) -> Result<Self, SnapshotError> {
        let mut reader = BufReader::new(File::open(path)?);
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let header: Value = serde_json::from_str(&line)
            .map_err(|error| SnapshotError::Invalid(error.to_string()))?;
        let invalid = || SnapshotError::Invalid("invalid Pi session header".into());
        if header["type"] != "session" || !matches!(header["version"].as_u64(), Some(2 | 3)) {
            return Err(invalid());
        }
        let id = header["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .ok_or_else(invalid)?
            .to_owned();
        let cwd = PathBuf::from(header["cwd"].as_str().ok_or_else(invalid)?);
        if !cwd.is_absolute() {
            return Err(invalid());
        }
        let created_at =
            DateTime::parse_from_rfc3339(header["timestamp"].as_str().ok_or_else(invalid)?)
                .map_err(|_| invalid())?
                .with_timezone(&Utc);
        let mut entries = Vec::new();
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            match serde_json::from_str::<Value>(&line) {
                Ok(entry) => entries.push(entry),
                // A live writer can be between writes. Never expose an incomplete record.
                Err(_) if !line.ends_with('\n') => break,
                Err(error) => return Err(SnapshotError::Invalid(error.to_string())),
            }
        }
        Ok(Self {
            id,
            cwd,
            created_at,
            entries,
        })
    }

    pub fn title(&self) -> String {
        // Pi's session name is file-wide metadata, independent of the selected branch.
        let name = self
            .entries
            .iter()
            .rev()
            .find(|entry| entry["type"] == "session_info")
            .and_then(|entry| entry["name"].as_str())
            .and_then(normalize_title);
        name.or_else(|| {
            self.entries.iter().find_map(|entry| {
                (entry["type"] == "message" && entry["message"]["role"] == "user")
                    .then(|| text_content(&entry["message"]["content"]))
                    .flatten()
                    .as_deref()
                    .and_then(normalize_title)
            })
        })
        .unwrap_or_else(|| fallback_title("Pi", &self.id))
    }

    pub fn session(&self, path: &Path, root: &Path) -> Option<AgentSession> {
        let locator = AgentSessionLocator {
            agent: "pi",
            session_id: self.id.clone(),
            title: self.title(),
            cwd: self.cwd.clone(),
            transcript_path: path.into(),
            trusted_root: root.into(),
        };
        snapshot::validate_locator(&locator).ok()?;
        let updated = std::fs::metadata(path)
            .ok()?
            .modified()
            .ok()
            .map(DateTime::<Utc>::from)?;
        Some(session(locator, self.created_at, updated))
    }

    pub fn branch(&self) -> Result<Vec<&Value>, SnapshotError> {
        let mut nodes = HashMap::new();
        for entry in &self.entries {
            let id = entry["id"]
                .as_str()
                .ok_or_else(|| SnapshotError::Invalid("Pi entry has no ID".into()))?;
            if nodes.insert(id, entry).is_some() {
                return Err(SnapshotError::Invalid("duplicate Pi entry ID".into()));
            }
        }
        let mut current = self.entries.last().and_then(|entry| entry["id"].as_str());
        let mut visited = HashSet::new();
        let mut branch = Vec::new();
        while let Some(id) = current {
            if !visited.insert(id) {
                return Err(SnapshotError::Invalid(
                    "cycle in Pi parentId ancestry".into(),
                ));
            }
            let entry = nodes
                .get(id)
                .ok_or_else(|| SnapshotError::Invalid("missing Pi parent entry".into()))?;
            branch.push(*entry);
            current = entry["parentId"].as_str();
        }
        branch.reverse();
        Ok(branch)
    }
}

pub(crate) fn text_content(content: &Value) -> Option<String> {
    let text = content.as_str().map(str::to_owned).unwrap_or_else(|| {
        content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|part| part["type"] == "text")
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    });
    (!text.trim().is_empty()).then_some(text)
}
