//! Translate ACP prompt completion into the existing page/IM notification pipeline.
use aow_acp::TurnCompletion;
use aow_agents::sessions::usage::TokenUsage;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    aow::AowManager,
    terminal::notifications::{TaskStopNotification, TaskStopSource},
};

pub(super) async fn dispatch(
    manager: &AowManager,
    page: &broadcast::Sender<TaskStopNotification>,
    completed: TurnCompletion,
) {
    let session = completed.session;
    let project_name = manager
        .project_name_for_workspace(&session.cwd)
        .await
        .unwrap_or_else(|| {
            std::path::Path::new(&session.cwd)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&session.cwd)
                .to_owned()
        });
    manager.notifications().dispatch(
        TaskStopNotification {
            agent: session.agent_id,
            session_id: session
                .session_id
                .unwrap_or_else(|| session.thread_id.clone()),
            title: session.title.clone(),
            cwd: session.cwd.clone(),
            turn_id: Some(completed.turn_id),
            conclusion: completed.conclusion,
            usage: completed.usage.as_ref().and_then(turn_usage),
            instance_ids: Vec::new(),
            sources: vec![TaskStopSource {
                project_name,
                workspace_root: session.cwd,
                tab_id: String::new(),
                thread_id: Some(session.thread_id),
                tab_name: session.title,
                tab_url: None,
            }],
        },
        page,
    );
}

fn turn_usage(usage: &Value) -> Option<TokenUsage> {
    Some(TokenUsage {
        input_tokens: usage["inputTokens"].as_u64()?,
        output_tokens: usage["outputTokens"].as_u64()?,
        total_tokens: usage["totalTokens"].as_u64()?,
        cached_input_tokens: usage["cachedReadTokens"].as_u64().unwrap_or(0),
        cache_write_input_tokens: usage["cachedWriteTokens"].as_u64().unwrap_or(0),
        reasoning_output_tokens: usage["thoughtTokens"].as_u64().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests;
