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
    let result = aow_i18n::spawn_blocking(move || -> Result<String> {
        let session = find_session(agent.id(), &session_id, roots).context(aow_i18n::text(
            "automations.no_session_record_found_for_this_run",
        ))?;
        agent
            .session_tracking()
            .context(aow_i18n::text(
                "automations.agent_does_not_support_reading_execution_results",
            ))?
            .completed_run_result(&session.locator(), exited_at)?
            .context(aow_i18n::text(
                "automations.no_final_task_response_retrieved",
            ))
    })
    .await
    .context(aow_i18n::text("automations.result_reading_interrupted"))
    .and_then(|result| result);
    store.write_run_result(&task.id, run_id, result)
}
