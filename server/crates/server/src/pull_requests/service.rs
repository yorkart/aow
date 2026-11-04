use super::{decode, positive, safe_path};
use crate::git_platform_providers::{
    GitPlatformError, GitPlatformProviderManager, GitPlatformQuery,
};
use aow_protocol::{MyPullRequests, PullRequestDetail, PullRequestDiff};
use serde_json::{Value, json};
use std::path::PathBuf;

fn invalid_json(message: impl ToString) -> GitPlatformError {
    GitPlatformError::InvalidJson(message.to_string())
}

pub(crate) async fn call(
    manager: &GitPlatformProviderManager,
    query: &GitPlatformQuery,
    operation: &str,
    params: Value,
    paths: &[PathBuf],
) -> Result<Value, GitPlatformError> {
    let invocation = manager
        .call(query, operation, params.clone(), paths)
        .await?;
    let result = invocation.result;
    let target = invocation.target;
    let mut result = match operation {
        "pull_requests.list" => {
            let mut data: MyPullRequests = decode(result)?;
            data.repository = query.repo.clone();
            for pr in &data.pull_requests {
                positive(pr.number).map_err(invalid_json)?;
            }
            serde_json::to_value(data).unwrap()
        }
        "pull_requests.detail" => {
            let data: PullRequestDetail = decode(result)?;
            if Some(data.summary.number) != params["number"].as_u64() {
                return Err(invalid_json("detail number differs from request"));
            }
            for file in &data.files {
                safe_path(&file.path).map_err(invalid_json)?;
            }
            serde_json::to_value(data).unwrap()
        }
        "pull_requests.diff" => {
            let mut data: PullRequestDiff = decode(result)?;
            if Some(data.number) != params["number"].as_u64()
                || Some(data.path.as_str()) != params["path"].as_str()
            {
                return Err(invalid_json("diff identity differs from request"));
            }
            if let Some(path) = &data.original_path {
                safe_path(path).map_err(invalid_json)?;
            }
            data.repository = query.repo.clone();
            serde_json::to_value(data).unwrap()
        }
        _ => {
            return Err(GitPlatformError::Invalid(
                aow_i18n::text("validation.unknown_operation").into(),
            ));
        }
    };
    let attach = |value: &mut Value| {
        value["provider"] = json!(target.provider);
        value["provider_name"] = json!(target.provider_name);
        value["remote"] = json!(target.remote);
    };
    attach(&mut result);
    if let Some(items) = result
        .get_mut("pull_requests")
        .and_then(Value::as_array_mut)
    {
        for item in items {
            attach(item);
        }
    }
    Ok(result)
}
