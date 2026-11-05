use crate::{EnvironmentError, RoleEnvironmentSpec};
use std::{collections::HashSet, path::PathBuf};
#[derive(Clone)]
pub struct EnvironmentManager {
    pub(crate) root: PathBuf,
}
impl EnvironmentManager {
    pub fn new(root: PathBuf) -> anyhow::Result<Self> {
        anyhow::ensure!(root.is_absolute(), "Node state directory must be absolute");
        Ok(Self { root })
    }
    pub fn validate(&self, spec: &RoleEnvironmentSpec) -> Result<(), EnvironmentError> {
        validate_id(&spec.id)?;
        crate::identity::validate(&spec.identity)?;
        crate::identity::files(spec.home_layout)?;
        if spec.skills.len() > 64 {
            return Err(EnvironmentError::Invalid("Too many skills".into()));
        }
        let mut names = HashSet::new();
        for source in &spec.skills {
            let path = std::path::Path::new(source);
            if !path.is_absolute() || !names.insert(path.file_name()) {
                return Err(EnvironmentError::Invalid(
                    "Skills must use unique absolute directories".into(),
                ));
            }
        }
        Ok(())
    }
}
pub(crate) fn validate_id(id: &str) -> Result<(), EnvironmentError> {
    if id.is_empty()
        || id.len() > 128
        || matches!(id, "." | "..")
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(EnvironmentError::Invalid("Invalid environment ID".into()));
    }
    Ok(())
}
