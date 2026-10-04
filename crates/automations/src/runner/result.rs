use anyhow::{Context, Result};
use aow_agents::sessions::{find_session, tracking::AgentSessionTracker};
use chrono::{DateTime, Utc};

use crate::{Store, Task};

/// Capture before Finished is published or a temporary worktree is removed.
/// Result availability is separate from the agent process's execution status.
pub(super) async fn capture(
    store: &Store,
    task: &Task,
    run_id: &str,
    session_id: String,
    exited_at: DateTime<Utc>,
) -> Result<()> {
    let roots = store.run_session_roots(&task.id, run_id)?;
    let agent = task.input.agent.agent();
    let result = tokio::task::spawn_blocking(move || -> Result<String> {
        let session =
            find_session(agent.id(), &session_id, roots).context("未找到本次运行的会话记录")?;
        agent
            .session_tracking()
            .context("Agent 不支持读取执行结果")?
            .completed_run_result(&session.locator(), exited_at)?
            .context("未获取到任务最终结论")
    })
    .await
    .context("执行结果读取中断")
    .and_then(|result| result);
    store.write_run_result(&task.id, run_id, result)
}
