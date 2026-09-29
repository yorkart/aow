use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use super::{
    persistence::{Persistence, read_optional},
    store::{TaskStore, identifier, invalid, revision, text},
};
use crate::HttpError;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RequirementSource {
    Inbox,
    RepositoryIssues {
        enabled: bool,
        provider: Option<String>,
        remote: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceSettings {
    pub revision: u64,
    pub sources: Vec<RequirementSource>,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            revision: 0,
            sources: vec![
                RequirementSource::Inbox,
                RequirementSource::RepositoryIssues {
                    enabled: true,
                    provider: None,
                    remote: None,
                },
            ],
        }
    }
}

impl SourceSettings {
    pub fn validate(&self) -> Result<(), HttpError> {
        if self.sources.len() != 2
            || self
                .sources
                .iter()
                .filter(|s| matches!(s, RequirementSource::Inbox))
                .count()
                != 1
            || self
                .sources
                .iter()
                .filter(|s| matches!(s, RequirementSource::RepositoryIssues { .. }))
                .count()
                != 1
        {
            return Err(invalid("需求源必须包含一个 Inbox 和一个仓库 Issue 来源"));
        }
        for source in &self.sources {
            if let RequirementSource::RepositoryIssues {
                provider, remote, ..
            } = source
            {
                if provider.is_some() != remote.is_some() {
                    return Err(invalid("Provider 和 remote 必须一起选择"));
                }
                if let Some(provider) = provider {
                    identifier(provider)?;
                }
                if let Some(remote) = remote {
                    text(remote, 256)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    projects: BTreeMap<String, SourceSettings>,
}

impl Persistence {
    pub fn load_sources(&self) -> anyhow::Result<BTreeMap<String, SourceSettings>> {
        let Some(doc) =
            read_optional::<Document>(&self.config.directory().join("tasks/sources.json"))?
        else {
            return Ok(BTreeMap::new());
        };
        anyhow::ensure!(doc.version == 1, "Unsupported requirement source format");
        for (project, settings) in &doc.projects {
            anyhow::ensure!(
                identifier(project).is_ok() && settings.validate().is_ok(),
                "Invalid requirement source configuration"
            );
        }
        Ok(doc.projects)
    }
}

impl TaskStore {
    pub(super) fn sources(&self, project: &str) -> Result<SourceSettings, HttpError> {
        identifier(project)?;
        Ok(self
            .lock()?
            .sources
            .get(project)
            .cloned()
            .unwrap_or_default())
    }

    pub(super) fn save_sources(
        &self,
        project: &str,
        mut settings: SourceSettings,
    ) -> Result<SourceSettings, HttpError> {
        identifier(project)?;
        settings.validate()?;
        let mut data = self.lock()?;
        revision(
            data.sources.get(project).map_or(0, |s| s.revision),
            settings.revision,
        )?;
        settings.revision = settings
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Source revision overflow"))?;
        let mut projects = data.sources.clone();
        projects.insert(project.into(), settings.clone());
        if let Some(persistence) = &self.persistence {
            persistence
                .save_config(
                    Path::new("tasks/sources.json"),
                    &Document {
                        version: 1,
                        projects: projects.clone(),
                    },
                )
                .map_err(|e| HttpError::internal(format!("{e:#}")))?;
        }
        data.sources = projects;
        drop(data);
        self.events.tasks_changed();
        Ok(settings)
    }
}
