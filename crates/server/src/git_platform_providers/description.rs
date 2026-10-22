use super::{GitPlatformProvider, Result, invalid_json, run_adapter};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Description {
    pub name: String,
    pub hosts: Vec<String>,
    pub operations: Vec<String>,
}
pub(super) async fn describe(
    provider: &GitPlatformProvider,
    paths: &[PathBuf],
) -> Result<Description> {
    describe_at(provider, paths, None).await
}
pub(super) async fn describe_at(
    provider: &GitPlatformProvider,
    paths: &[PathBuf],
    script_path: Option<&Path>,
) -> Result<Description> {
    let value = run_adapter(provider, json!({"version":super::schema::VERSION,"operation":"describe","repository":null,"params":{}}), None, paths, script_path).await?;
    let mut result: Description = serde_json::from_value(value).map_err(invalid_json)?;
    result.name = result.name.trim().to_owned();
    result.hosts = result
        .hosts
        .iter()
        .map(|host| host.trim().to_ascii_lowercase())
        .collect();
    if result.operations.iter().collect::<HashSet<_>>().len() != result.operations.len()
        || result.hosts.iter().collect::<HashSet<_>>().len() != result.hosts.len()
    {
        return Err(invalid_json(
            "describe hosts and operations must contain unique values",
        ));
    }
    if result.operations.is_empty()
        || result
            .operations
            .iter()
            .any(|op| !super::schema::OPERATIONS.contains(&op.as_str()))
    {
        return Err(invalid_json(
            "describe must declare at least one supported read operation",
        ));
    }
    let check = GitPlatformProvider {
        name: result.name.clone(),
        hosts: result.hosts.clone(),
        ..provider.clone()
    };
    super::validate_settings(&super::Settings {
        revision: 0,
        providers: vec![check],
    })?;
    Ok(result)
}
