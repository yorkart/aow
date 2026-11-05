use super::{GitPlatformError, GitPlatformProvider, Result, invalid_json};
use serde_json::Value;
use std::path::{Path, PathBuf};
pub(super) async fn git(
    runtime: Option<&aow_runtime_client::RuntimeClient>,
    repo: &str,
    args: &[&str],
    paths: &[PathBuf],
) -> Result<String> {
    #[cfg(test)]
    let fixture = if runtime.is_none() {
        Some(super::test_node::start(paths).await)
    } else {
        None
    };
    #[cfg(test)]
    let runtime = runtime.or_else(|| fixture.as_ref().map(|(_, client)| client));

    if let Some(runtime) = runtime {
        let output = runtime
            .workspace_git(Path::new(repo), args)
            .await
            .map_err(|e| GitPlatformError::Command(e.to_string()))?;
        if !output.success {
            return Err(GitPlatformError::Command(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        return String::from_utf8(output.stdout).map_err(invalid_json);
    }

    {
        let _ = paths;
        Err(GitPlatformError::Unavailable(
            "Workspace runtime is unavailable".into(),
        ))
    }
}
pub(super) async fn run_adapter(
    runtime: Option<&aow_runtime_client::RuntimeClient>,
    provider: &GitPlatformProvider,
    request: Value,
    cwd: Option<&Path>,
    paths: &[PathBuf],
    script_path: Option<&Path>,
) -> Result<Value> {
    #[cfg(test)]
    let fixture = if runtime.is_none() {
        Some(super::test_node::start(paths).await)
    } else {
        None
    };
    #[cfg(test)]
    let runtime = runtime.or_else(|| fixture.as_ref().map(|(_, client)| client));

    if !super::validation::valid_filename(&provider.id)
        || provider.script.len() > super::SCRIPT_LIMIT
    {
        return Err(super::invalid_json(
            "upload a single .mjs provider using protocol v4",
        ));
    }
    if request["operation"]
        .as_str()
        .is_none_or(|op| op != "describe" && !super::schema::OPERATIONS.contains(&op))
    {
        return Err(super::invalid_json(
            "unsupported Git platform read operation",
        ));
    }

    let value: Value = if let Some(runtime) = runtime {
        runtime
            .workspace_provider(&aow_runtime_contract::workspace::ProviderRequest {
                filename: provider.id.clone(),
                script: provider.script.clone(),
                request,
                cwd: cwd.map(Path::to_path_buf),
            })
            .await
            .map_err(|error| GitPlatformError::Command(error.to_string()))?
    } else {
        {
            let _ = (paths, script_path);
            return Err(GitPlatformError::Unavailable(
                "Workspace runtime is unavailable".into(),
            ));
        }
    };
    if value.get("version").and_then(Value::as_u64) != Some(super::schema::VERSION) {
        return Err(invalid_json(
            "expected version: 4; upload a Node.js .mjs provider using the current Git platform protocol",
        ));
    }
    match (value.get("result"), value.get("error")) {
        (Some(result), None) if result.is_object() => Ok(result.clone()),
        (None, Some(error)) if error["code"].is_string() && error["message"].is_string() => {
            Err(GitPlatformError::Command(format!(
                "{}: {}",
                error["code"].as_str().unwrap(),
                error["message"].as_str().unwrap()
            )))
        }
        _ => Err(invalid_json(
            "expected either result object or error {code, message}",
        )),
    }
}
