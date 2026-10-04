use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::{Store, atomic_write};
use crate::{Run, RunStatus};

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RunResult {
    Completed { conclusion: String },
    Unavailable { error: String },
}

impl Store {
    pub(crate) fn write_run_result(
        &self,
        task_id: &str,
        run_id: &str,
        result: Result<String>,
    ) -> Result<()> {
        let result = match result {
            Ok(conclusion) => RunResult::Completed { conclusion },
            Err(error) => RunResult::Unavailable {
                error: format!("{error:#}"),
            },
        };
        atomic_write(
            &self.run_path(task_id, run_id)?.join("result.json"),
            &serde_json::to_vec(&result)?,
        )
    }

    /// Immutable result captured by the runner, independent of native history
    /// availability or later session resumes and environment changes.
    pub fn read_run_result(&self, run: &Run) -> Result<String> {
        ensure!(run.status == RunStatus::Completed, "任务尚未成功结束");
        let bytes = std::fs::read(self.run_path(&run.task_id, &run.id)?.join("result.json"))
            .context("本次运行没有保存执行结果")?;
        match serde_json::from_slice(&bytes)? {
            RunResult::Completed { conclusion } => Ok(conclusion),
            RunResult::Unavailable { error } => bail!("执行结果收集失败：{error}"),
        }
    }
}
