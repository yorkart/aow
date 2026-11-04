use super::{GitPlatformError, GitPlatformProvider, Result, invalid_json};
use serde_json::Value;
use std::path::{Path, PathBuf};
#[cfg(test)]
pub(super) const OUTPUT_LIMIT: usize = 16 * 1024 * 1024;
pub(super) async fn git(
    runtime: Option<&aow_runtime_client::RuntimeClient>,
    repo: &str,
    args: &[&str],
    paths: &[PathBuf],
) -> Result<String> {
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
    #[cfg(test)]
    {
        super::adapter_fixture::git(repo, args, paths).await
    }
    #[cfg(not(test))]
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
        runtime.post_json("/v1/workspace/provider",&serde_json::json!({"filename":provider.id,"script":provider.script,"request":request,"cwd":cwd})).await.map_err(|error|GitPlatformError::Command(error.to_string()))?
    } else {
        #[cfg(test)]
        {
            return super::adapter_fixture::run_adapter(provider, request, cwd, paths, script_path)
                .await;
        }
        #[cfg(not(test))]
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
