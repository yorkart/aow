use super::{
    AgentRole,
    model::{RoleConfig, RoleRuntime},
};
use crate::aow::{AowError, AowManager};
use std::{
    collections::{BTreeMap, HashSet},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn invalid(message: impl Into<String>) -> AowError {
    AowError::Invalid(message.into())
}

pub(super) fn validate(config: &RoleConfig) -> Result<(), AowError> {
    crate::aow::validation::validate_display_name(&config.name)?;
    super::identity::validate(&config.identity)?;
    super::identity::files(config.home_layout)?;
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
            // Reuse ACP's ID rules for configured adapters.
            aow_acp::validate_settings(
                &serde_json::json!({"agent_servers":{(agent_id):{"type":"registry"}}}).to_string(),
            )
            .map_err(|error| invalid(error.to_string()))?;
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

fn absolute_directory(value: &str) -> Result<PathBuf, AowError> {
    let path = Path::new(value);
    if !path.is_absolute() || !path.is_dir() {
        return Err(invalid(
            "Skill sources must be existing absolute directories",
        ));
    }
    Ok(path.canonicalize()?)
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

pub(super) fn private_directory(path: &Path) -> Result<(), AowError> {
    if path.ancestors().any(|ancestor| {
        std::fs::symlink_metadata(ancestor).is_ok_and(|meta| meta.file_type().is_symlink())
    }) {
        return Err(invalid("Role home directories cannot be symbolic links"));
    }
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn copy_skill_directory(source: &Path, destination: &Path) -> Result<(), AowError> {
    let source = source.canonicalize()?;
    let mut budget = (0_usize, 0_u64);
    copy_skill_tree(
        &source,
        &source,
        destination,
        &mut HashSet::new(),
        &mut budget,
    )
}

fn copy_skill_tree(
    root: &Path,
    source: &Path,
    destination: &Path,
    visiting: &mut HashSet<PathBuf>,
    budget: &mut (usize, u64),
) -> Result<(), AowError> {
    let source = source.canonicalize()?;
    if !source.starts_with(root) || !visiting.insert(source.clone()) {
        return Err(invalid(
            "Skill source contains an external or cyclic symbolic link",
        ));
    }
    private_directory(destination)?;
    for entry in std::fs::read_dir(&source)? {
        let entry = entry?;
        if matches!(
            entry.file_name().to_str(),
            Some("sessions" | "logs" | "tmp" | "cache" | ".cache" | "node_modules" | ".git")
        ) {
            continue;
        }
        let original = entry.path();
        // Sockets and devices are not skill assets.
        let metadata = original.metadata().map_err(|error| {
            invalid(format!(
                "Cannot read skill source {}: {error}",
                original.display()
            ))
        })?;
        if !metadata.is_dir() && !metadata.is_file() {
            continue;
        }
        let path = original.canonicalize()?;
        if !path.starts_with(root) {
            return Err(invalid(format!(
                "Skill source contains a symbolic link outside its directory: {}",
                original.display()
            )));
        }
        budget.0 += 1;
        if budget.0 > 50000 {
            return Err(invalid("Skill source contains too many files"));
        }
        let output = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_skill_tree(root, &path, &output, visiting, budget)?;
        } else if metadata.is_file() {
            budget.1 += metadata.len();
            if budget.1 > 512 * 1024 * 1024 {
                return Err(invalid("Skill source exceeds 512 MiB"));
            }
            std::fs::copy(&path, &output)?;
            std::fs::set_permissions(
                output,
                std::fs::Permissions::from_mode(if metadata.permissions().mode() & 0o111 != 0 {
                    0o700
                } else {
                    0o600
                }),
            )?;
        }
    }
    visiting.remove(&source);
    Ok(())
}

/// Roll back file changes if the corresponding configuration cannot be committed.
#[derive(Default)]
pub(super) struct PreparedHome {
    created_home: Option<PathBuf>,
    replacements: Vec<(PathBuf, Option<PathBuf>)>,
    committed: bool,
}

impl PreparedHome {
    fn replace(
        &mut self,
        root: &Path,
        destination: PathBuf,
        staging: Option<PathBuf>,
    ) -> Result<(), AowError> {
        let backup = if std::fs::symlink_metadata(&destination).is_ok() {
            let backup = root.join(format!(".backup-{}", aow_id::new_id()));
            std::fs::rename(&destination, &backup)?;
            Some(backup)
        } else {
            None
        };
        self.replacements.push((destination.clone(), backup));
        if let Some(staging) = staging {
            std::fs::rename(staging, destination)?;
        }
        Ok(())
    }

    pub(super) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for PreparedHome {
    fn drop(&mut self) {
        for (destination, backup) in self.replacements.iter().rev() {
            if !self.committed {
                if destination.exists() {
                    let _ = std::fs::remove_dir_all(destination);
                }
                if let Some(backup) = backup
                    && let Err(error) = std::fs::rename(backup, destination)
                {
                    tracing::error!(%error, "Failed to restore role skill directory");
                }
            } else if let Some(backup) = backup {
                let _ = std::fs::remove_dir_all(backup);
            }
        }
        if !self.committed
            && let Some(home) = &self.created_home
        {
            let _ = std::fs::remove_dir_all(home);
        }
    }
}

impl AowManager {
    pub(crate) fn role_home(&self, id: &str) -> Result<PathBuf, AowError> {
        crate::aow::validation::validate_id(id)?;
        let state = self
            .inner
            .state_dir
            .as_ref()
            .ok_or_else(|| invalid("Agent roles require persistent storage"))?;
        Ok(state.join("data/agent-roles").join(id).join("home"))
    }

    pub(super) fn prepare_role_home(
        &self,
        role: &AgentRole,
        previous: Option<&AgentRole>,
    ) -> Result<PreparedHome, AowError> {
        let mut changes = PreparedHome::default();
        let home = self.role_home(&role.id)?;
        let root = home.parent().ok_or_else(|| invalid("Invalid role home"))?;
        private_directory(root)?;
        if !home.exists() {
            if previous.is_some() {
                return Err(invalid(
                    "Role home is missing; restore it before changing the role",
                ));
            }
            let staging = root.join(format!(".seed-{}", aow_id::new_id()));
            let result = (|| {
                private_directory(&staging)?;
                super::identity::import(&role.config.identity, role.config.home_layout, &staging)?;
                std::fs::rename(&staging, &home)?;
                Ok::<_, AowError>(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_dir_all(staging);
            }
            result?;
            changes.created_home = Some(home.clone());
        }
        private_directory(&home)?;
        if previous.is_none_or(|previous| previous.config.skills != role.config.skills)
            && (!role.config.skills.is_empty()
                || previous.is_some_and(|previous| !previous.config.skills.is_empty()))
        {
            let skills = home.join(role.config.home_layout.skills_directory());
            private_directory(&skills)?;
            let mut names = HashSet::new();
            for source in &role.config.skills {
                let name = skill_name(source)?;
                names.insert(name.clone());
                if previous.is_some_and(|role| role.config.skills.contains(source)) {
                    continue;
                }
                let source_path = absolute_directory(source)?;
                if !source_path.join("SKILL.md").is_file() {
                    return Err(invalid("Each skill directory must contain SKILL.md"));
                }
                let staging = root.join(format!(".skill-{}", aow_id::new_id()));
                if let Err(error) = copy_skill_directory(Path::new(source), &staging) {
                    let _ = std::fs::remove_dir_all(staging);
                    return Err(error);
                }
                let destination = skills.join(&name);
                changes.replace(root, destination, Some(staging))?;
            }
            if let Some(previous) = previous {
                for source in &previous.config.skills {
                    let name = skill_name(source)?;
                    if !names.contains(&name) && skills.join(&name).exists() {
                        changes.replace(root, skills.join(name), None)?;
                    }
                }
            }
        }
        Ok(changes)
    }

    pub(crate) fn role_environment(
        &self,
        role: &AgentRole,
    ) -> Result<BTreeMap<String, String>, AowError> {
        let home = self.role_home(&role.id)?;
        if !home.is_dir() || home.symlink_metadata()?.file_type().is_symlink() {
            return Err(invalid("Role home is missing or is a symbolic link"));
        }
        let mut env = BTreeMap::from([
            ("HOME".into(), home.to_string_lossy().into_owned()),
            ("AOW_AGENT_ROLE_ID".into(), role.id.clone()),
        ]);
        for (key, relative) in [
            ("CODEX_HOME", ".codex"),
            ("CLAUDE_CONFIG_DIR", ".claude"),
            ("PI_CODING_AGENT_DIR", ".pi/agent"),
            ("PI_CODING_AGENT_SESSION_DIR", ".pi/agent/sessions"),
            ("HERMES_HOME", ".hermes"),
            ("TRAECLI_HOME", ".trae/cli"),
            ("TRAE_HOME", ".trae"),
            ("GEMINI_CLI_HOME", "."),
            ("XDG_CONFIG_HOME", ".config"),
            ("XDG_DATA_HOME", ".local/share"),
            ("XDG_CACHE_HOME", ".cache"),
            ("XDG_STATE_HOME", ".local/state"),
        ] {
            env.insert(
                key.into(),
                home.join(relative).to_string_lossy().into_owned(),
            );
        }
        Ok(env)
    }
}
