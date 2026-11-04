use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::Store;
use crate::{Run, RunStatus};

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RunResult {
    Completed { conclusion: String },
    Unavailable { error: String },
}

impl Store {
    /// Immutable result captured by the runner, independent of native history
    /// availability or later session resumes and environment changes.
    pub fn read_run_result(&self, run: &Run) -> Result<String> {
        ensure!(
            run.status == RunStatus::Completed,
            aow_i18n::message("automations.task_has_not_completed_successfully", &[])
        );
        let bytes = std::fs::read(self.run_path(&run.task_id, &run.id)?.join("result.json"))
            .context(aow_i18n::text(
                "automations.no_execution_result_saved_for_this_run",
            ))?;
        match serde_json::from_slice(&bytes)? {
            RunResult::Completed { conclusion } => Ok(conclusion),
            RunResult::Unavailable { error } => bail!(aow_i18n::message(
                "automations.failed_to_collect_execution_result",
                &[("p0", error.to_string())]
            )),
        }
    }
}
