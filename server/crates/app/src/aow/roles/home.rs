//! Role metadata validation; filesystem preparation belongs to the node.
use super::{
    AgentRole,
    model::{RoleConfig, RoleRuntime},
};
use crate::aow::{AowError, AowManager};
use std::{collections::HashSet, path::Path};
fn invalid(message: impl Into<String>) -> AowError {
    AowError::Invalid(message.into())
}
pub(super) fn validate(config: &RoleConfig) -> Result<(), AowError> {
    crate::aow::validation::validate_display_name(&config.name)?;
    super::identity::validate(&config.identity)?;
    if config.description.len() > 4096 || config.description.contains('\0') {
        return Err(invalid("Role description exceeds the supported size"));
    }
    if config.instruction.len() > 131_072
        || config.instruction.contains('\0')
        || config.skills.len() > 64
    {
        return Err(invalid(
            "Role instructions or skills exceed the supported size",
        ));
    }
    match &config.runtime {
        RoleRuntime::Acp { agent_id } => {
            if agent_id.is_empty()
                || agent_id.len() > 128
                || matches!(agent_id.as_str(), "." | "..")
                || !agent_id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            {
                return Err(invalid("Invalid runtime identifier"));
            }
        }
        RoleRuntime::Cli { agent_id } => {
            crate::aow::validation::validate_id(agent_id)?;
        }
    }
    let mut names = HashSet::new();
    for skill in &config.skills {
        if !Path::new(skill).is_absolute() || !names.insert(skill_name(skill)?) {
            return Err(invalid(
                "Skills must contain SKILL.md and have unique directory names",
            ));
        }
    }
    Ok(())
}

fn skill_name(value: &str) -> Result<String, AowError> {
    let name = Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if name.is_empty()
        || name.starts_with('.')
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err(invalid(
            "Skill directory names must use letters, digits, hyphens, dots or underscores",
        ));
    }
    Ok(name.into())
}

impl AowManager {
    pub(crate) fn role_home(&self, id: &str) -> Result<std::path::PathBuf, AowError> {
        self.inner
            .roles
            .environments
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .get(id)
            .map(|environment| environment.home.clone())
            .ok_or_else(|| invalid("Resolve the role environment on the node first"))
    }
    pub(crate) fn role_environment(
        &self,
        role: &AgentRole,
    ) -> Result<std::collections::BTreeMap<String, String>, AowError> {
        self.inner
            .roles
            .environments
            .lock()
            .map_err(|_| AowError::Poisoned)?
            .get(&role.id)
            .map(|environment| environment.env.clone())
            .ok_or_else(|| invalid("Resolve the role environment on the node first"))
    }
}
