use super::ThreadMetadata;
use crate::facade::SessionSource;
use anyhow::{Result, bail};
use aow_db::acp::ThreadRecord;

impl TryFrom<ThreadRecord> for ThreadMetadata {
    type Error = anyhow::Error;

    fn try_from(row: ThreadRecord) -> Result<Self> {
        Ok(Self {
            thread_id: row.thread_id,
            source: match row.source.as_str() {
                "user" => SessionSource::User,
                "cli" => SessionSource::Cli,
                source => bail!("Unsupported ACP thread source: {source}"),
            },
            parent: row
                .parent
                .map(|value| serde_json::from_str(&value))
                .transpose()?,
            session_id: row.session_id,
            agent_id: row.agent_id,
            title: row.title,
            updated_at: row.updated_at,
            created_at: row.created_at,
            interacted_at: row.interacted_at,
            folder_paths: row
                .folder_paths
                .map(|value| serde_json::from_str(&value))
                .transpose()?
                .unwrap_or_default(),
            archived: row.archived,
            title_override: row.title_override,
        })
    }
}

impl ThreadMetadata {
    pub(super) fn into_record(self) -> Result<ThreadRecord> {
        Ok(ThreadRecord {
            thread_id: self.thread_id,
            source: match self.source {
                SessionSource::User => "user",
                SessionSource::Cli => "cli",
            }
            .into(),
            parent: self
                .parent
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            session_id: self.session_id,
            agent_id: self.agent_id,
            title: self.title,
            updated_at: self.updated_at,
            created_at: self.created_at,
            interacted_at: self.interacted_at,
            folder_paths: Some(serde_json::to_string(&self.folder_paths)?),
            archived: self.archived,
            title_override: self.title_override,
        })
    }
}
