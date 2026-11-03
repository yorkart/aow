use super::{TaskDb, WakeupDelivery};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSource {
    pub task_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
    pub request: String,
    pub parent_run_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRun {
    pub id: String,
    pub request_id: String,
    pub task_id: String,
    pub project_id: String,
    pub thread_id: String,
    pub prompt: String,
    pub source: Option<RunSource>,
    pub state: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub attempts: u32,
    pub retry_at: i64,
}

impl TaskRun {
    pub fn terminal(&self) -> bool {
        matches!(
            self.state.as_str(),
            "result_available" | "failed" | "interrupted"
        )
    }
}

pub(super) fn run(connection: &Connection, id: &str) -> Result<Option<TaskRun>> {
    let data: Option<String> = connection
        .query_row("SELECT payload FROM task_runs WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    data.map(|data| serde_json::from_str(&data).map_err(Into::into))
        .transpose()
}

pub(super) fn insert_run(connection: &Connection, run: &TaskRun) -> Result<()> {
    connection.execute(
        "INSERT INTO task_runs (id, task_id, thread_id, parent_run_id, state, payload, request_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![run.id, run.task_id, run.thread_id, run.source.as_ref().and_then(|s| s.parent_run_id.as_deref()), run.state, serde_json::to_string(run)?, run.request_id],
    )?;
    Ok(())
}

pub(super) fn save_run(connection: &Connection, run: &TaskRun) -> Result<()> {
    connection.execute(
        "UPDATE task_runs SET state = ?2, payload = ?3 WHERE id = ?1",
        params![run.id, run.state, serde_json::to_string(run)?],
    )?;
    Ok(())
}

fn request_run(
    connection: &Connection,
    task_id: &str,
    request_id: &str,
) -> Result<Option<TaskRun>> {
    let data: Option<String> = connection
        .query_row(
            "SELECT payload FROM task_runs WHERE task_id = ?1 AND request_id = ?2",
            params![task_id, request_id],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| serde_json::from_str(&data).map_err(Into::into))
        .transpose()
}

pub(super) fn children_pending(connection: &Connection, id: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM task_runs WHERE parent_run_id = ?1 AND state IN ('queued', 'running', 'waiting'))
         OR EXISTS(SELECT 1 FROM task_wakeups WHERE continuation_run_id = ?1 AND state != 'processed')",
        [id], |row| row.get(0),
    )?)
}

/// Freeze one execution result and its human/machine deliveries in one commit.
pub(super) fn finish(connection: &Connection, run: &mut TaskRun, now: i64) -> Result<()> {
    if run.state == "result_available" && children_pending(connection, &run.id)? {
        run.state = "waiting".into();
        return save_run(connection, run);
    }
    run.finished_at = Some(now);
    save_run(connection, run)?;
    let event_id = format!("result-{}", run.id);
    connection.execute(
        "INSERT INTO task_inbox (id, run_id, project_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![event_id, run.id, run.project_id, now],
    )?;
    if let Some(source) = &run.source {
        let delivery = WakeupDelivery {
            id: event_id.clone(),
            run_id: run.id.clone(),
            thread_id: source.thread_id.clone(),
            continuation_run_id: source.parent_run_id.clone(),
            prompt_id: format!("wakeup-{event_id}"),
            state: "pending".into(),
            attempts: 0,
            retry_at: 0,
            error: None,
        };
        connection.execute(
            "INSERT INTO task_wakeups (id, prompt_id, continuation_run_id, state, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![delivery.id, delivery.prompt_id, delivery.continuation_run_id, delivery.state, serde_json::to_string(&delivery)?],
        )?;
    }
    Ok(())
}

impl TaskDb {
    pub fn run_for_request(&self, task_id: &str, request_id: &str) -> Result<Option<TaskRun>> {
        request_run(&*self.lock()?, task_id, request_id)
    }

    pub fn run(&self, id: &str) -> Result<Option<TaskRun>> {
        run(&*self.lock()?, id)
    }

    pub fn enqueue_run(&self, request: &TaskRun) -> Result<TaskRun> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        if let Some(existing) = request_run(&tx, &request.task_id, &request.request_id)? {
            ensure!(
                existing.task_id == request.task_id
                    && existing.prompt == request.prompt
                    && existing.source.as_ref().map(|s| &s.thread_id)
                        == request.source.as_ref().map(|s| &s.thread_id),
                "Request ID already used for different work"
            );
            return Ok(existing);
        }
        // An executor must not wait for a return to an ancestor that is itself
        // waiting on this executor; ordinary unrelated tasks may communicate.
        let mut parent = request
            .source
            .as_ref()
            .and_then(|s| s.parent_run_id.clone());
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = parent {
            ensure!(seen.insert(id.clone()), "Delegation cycle");
            let Some(ancestor) = run(&tx, &id)? else {
                break;
            };
            ensure!(
                ancestor.thread_id != request.thread_id,
                "Cannot delegate back to a waiting ancestor"
            );
            parent = ancestor.source.and_then(|s| s.parent_run_id);
        }
        insert_run(&tx, request)?;
        tx.commit()?;
        Ok(request.clone())
    }

    pub fn active_runs(&self) -> Result<Vec<TaskRun>> {
        let connection = self.lock()?;
        let mut query = connection.prepare(
            "SELECT payload FROM task_runs WHERE state IN ('queued', 'running') ORDER BY rowid",
        )?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter()
            .map(|data| serde_json::from_str(data).map_err(Into::into))
            .collect()
    }

    pub fn task_runs(&self, task_id: &str) -> Result<Vec<TaskRun>> {
        let connection = self.lock()?;
        let mut query = connection.prepare(
            "SELECT payload FROM task_runs WHERE task_id = ?1 ORDER BY rowid DESC LIMIT 100",
        )?;
        let rows = query
            .query_map([task_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter()
            .map(|data| serde_json::from_str(data).map_err(Into::into))
            .collect()
    }

    pub fn update_run_admission(
        &self,
        id: &str,
        state: &str,
        error: Option<String>,
        retry_at: i64,
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut run = run(&tx, id)?.ok_or_else(|| anyhow::anyhow!("Run not found"))?;
        if matches!(run.state.as_str(), "queued" | "running") {
            run.state = state.into();
            if error.is_some() {
                run.attempts += 1;
            }
            run.error = error;
            run.retry_at = retry_at;
            save_run(&tx, &run)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn finish_run(
        &self,
        id: &str,
        status: &str,
        result: Option<String>,
        error: Option<String>,
        now: i64,
    ) -> Result<()> {
        ensure!(
            matches!(status, "result_available" | "failed" | "interrupted"),
            "Invalid run outcome"
        );
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut run = run(&tx, id)?.ok_or_else(|| anyhow::anyhow!("Run not found"))?;
        if !run.terminal() {
            run.state = status.into();
            run.result = result;
            run.error = error;
            finish(&tx, &mut run, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The active managed prompt identifies a continuation, never the task's
    /// original requester. A later send can have a different return address.
    pub fn run_for_prompt(&self, prompt_id: &str) -> Result<Option<String>> {
        let connection = self.lock()?;
        if let Some(run) = run(&connection, prompt_id)? {
            return Ok((!run.terminal()).then_some(run.id));
        }
        Ok(connection
            .query_row(
                "SELECT continuation_run_id FROM task_wakeups WHERE prompt_id = ?1",
                [prompt_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    pub fn owns_prompt(&self, prompt_id: &str) -> Result<bool> {
        Ok(self.lock()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_runs WHERE id = ?1) OR EXISTS(SELECT 1 FROM task_wakeups WHERE prompt_id = ?1)",
            [prompt_id], |row| row.get(0))?)
    }
}
