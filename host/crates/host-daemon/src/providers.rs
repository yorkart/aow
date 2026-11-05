//! Git hosting adapters execute beside the repository, under node process ownership.
use aow_host_contract::workspace::ProviderRequest as Request;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::post,
};
use std::{collections::BTreeMap, path::PathBuf};
pub(crate) fn router(root: PathBuf) -> Router {
    Router::new()
        .route("/v1/workspace/provider", post(invoke))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(root)
}
async fn invoke(
    State(root): State<PathBuf>,
    Json(request): Json<Request>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    async {
        anyhow::ensure!(
            request.filename.len() <= 96
                && request.filename.ends_with(".mjs")
                && request
                    .filename
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                && request.script.len() <= 1024 * 1024,
            "Invalid Git platform adapter"
        );
        anyhow::ensure!(
            request.request["version"] == 4,
            "Unsupported Git platform protocol"
        );
        let paths = aow_agents_core::environment::load_path_from(&root).await?;
        let node = aow_agents_core::management::executable("node", &paths)
            .ok_or_else(|| anyhow::anyhow!("Node.js is unavailable on the execution node"))?;
        let parent = root.join("cache/git-platform-providers");
        tokio::fs::create_dir_all(&parent).await?;
        let temporary = tempfile::Builder::new()
            .prefix("invocation-")
            .tempdir_in(parent)?;
        let script = temporary.path().join(&request.filename);
        tokio::fs::write(&script, &request.script).await?;
        let environment = BTreeMap::from([(
            "PATH".into(),
            aow_agents_core::environment::path_value(&paths)?,
        )]);
        let result = aow_process::run_once(aow_process::ProcessSpec {
            executable: node.into(),
            arguments: vec![script.to_string_lossy().into_owned()],
            cwd: request.cwd.unwrap_or_else(|| temporary.path().into()),
            environment,
            stdin: serde_json::to_vec(&request.request)?,
            timeout_ms: 60_000,
            max_output_bytes: 16 * 1024 * 1024,
        })
        .await?;
        anyhow::ensure!(
            result.success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
                .chars()
                .take(2000)
                .collect::<String>()
        );
        Ok(Json(serde_json::from_slice(&result.stdout)?))
    }
    .await
    .map_err(|error: anyhow::Error| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"code":"git_provider_failed","message":error.to_string()})),
        )
    })
}
