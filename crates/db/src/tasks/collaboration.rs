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
pub struct RunExecution {
    pub agent_id: String,
    pub agent_role_id: Option<String>,
    pub workspace: String,
    pub workspace_intent: Option<String>,
    pub target_task_id: Option<String>,
    pub target_revision: Option<String>,
}

impl From<&super::TaskRecord> for RunExecution {
    fn from(task: &super::TaskRecord) -> Self {
        Self {
            agent_id: task.agent_id.clone(),
            agent_role_id: task.agent_role_id.clone(),
            workspace: task.workspace.clone(),
            workspace_intent: task.workspace_intent.clone(),
            target_task_id: task.target_task_id.clone(),
            target_revision: task.target_revision.clone(),
        }
    }
}

fn message_trigger() -> String {
    "message".into()
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
    #[serde(default)]
    pub execution: Option<RunExecution>,
    #[serde(default = "message_trigger")]
    pub trigger: String,
    #[serde(default)]
    pub caused_by_run_id: Option<String>,
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
    connection.execute(
        "UPDATE tasks SET status = 'in_progress' WHERE id = ?1 AND status = 'todo'",
        [&run.task_id],
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

/// Freeze one execution result and its human/machine deliveries in one commit.
pub(super) fn finish(connection: &Connection, run: &mut TaskRun, now: i64) -> Result<()> {
    run.finished_at = Some(now);
    save_run(connection, run)?;
    // A root wakeup consumes an existing Inbox event. Keep its execution history
    // without creating a second notification for the same result.
    if run.trigger == "wakeup" && run.source.is_none() {
        return Ok(());
    }
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
        self.enqueue(request, false)
    }

    pub fn enqueue_task_run(&self, request: &TaskRun) -> Result<TaskRun> {
        self.enqueue(request, true)
    }

    fn enqueue(&self, request: &TaskRun, attach: bool) -> Result<TaskRun> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        if let Some(existing) = request_run(&tx, &request.task_id, &request.request_id)? {
            ensure!(
                existing.task_id == request.task_id
                    && existing.prompt == request.prompt
                    && existing.execution == request.execution
                    && existing.thread_id == request.thread_id
                    && existing.source.as_ref().map(|s| &s.thread_id)
                        == request.source.as_ref().map(|s| &s.thread_id),
                "Request ID already used for different work"
            );
            return Ok(existing);
        }
        // Provenance is acyclic. Sending back to an active ancestor's thread is
        // allowed: runs no longer wait for children and the message is queued.
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
            parent = ancestor.source.and_then(|s| s.parent_run_id);
        }
        if attach {
            let owner: Option<String> = tx
                .query_row(
                    "SELECT task_id FROM task_resources WHERE kind = 'acp' AND resource_id = ?1",
                    [&request.thread_id],
                    |row| row.get(0),
                )
                .optional()?;
            ensure!(
                owner.is_none_or(|owner| owner == request.task_id),
                "Conversation belongs to another task"
            );
            tx.execute(
                "INSERT OR IGNORE INTO task_resources VALUES (?1, 'acp', ?2)",
                params![request.task_id, request.thread_id],
            )?;
        }
        insert_run(&tx, request)?;
        tx.commit()?;
        Ok(request.clone())
    }

    pub fn active_runs(&self) -> Result<Vec<TaskRun>> {
        let connection = self.lock()?;
        let mut query = connection.prepare(
            "SELECT payload FROM task_runs WHERE state IN ('queued', 'running') AND COALESCE(json_extract(payload, '$.trigger'), 'message') != 'wakeup' ORDER BY rowid",
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

    /// A wakeup is its own execution, never a continuation of an old run.
    pub fn run_for_prompt(&self, prompt_id: &str) -> Result<Option<String>> {
        let connection = self.lock()?;
        if let Some(run) = run(&connection, prompt_id)? {
            return Ok((!run.terminal()).then_some(run.id));
        }
        Ok(None)
    }

    pub fn thread_execution(&self, thread: &str) -> Result<Option<RunExecution>> {
        let connection = self.lock()?;
        let data: Option<String> = connection
            .query_row(
                "SELECT payload FROM task_runs WHERE thread_id = ?1 ORDER BY rowid DESC LIMIT 1",
                [thread],
                |row| row.get(0),
            )
            .optional()?;
        Ok(data
            .map(|data| serde_json::from_str::<TaskRun>(&data))
            .transpose()?
            .and_then(|run| run.execution))
    }

    pub fn execution_workspace(&self, task: &str, intent: &str) -> Result<Option<String>> {
        Ok(self.lock()?.query_row("SELECT json_extract(payload, '$.execution.workspace') FROM task_runs WHERE task_id = ?1 AND json_extract(payload, '$.execution.workspace_intent') = ?2 ORDER BY rowid DESC LIMIT 1", params![task, intent], |row| row.get(0)).optional()?)
    }

    pub fn owns_workspace(&self, task: &str, workspace: &str) -> Result<bool> {
        Ok(self.lock()?.query_row("SELECT EXISTS(SELECT 1 FROM task_runs WHERE task_id = ?1 AND json_extract(payload, '$.execution.workspace') = ?2)", params![task, workspace], |row| row.get(0))?)
    }

    /// Old 'waiting' runs already ended their ACP turn. Freeze that original
    /// result once; pending returns will create separate wakeup runs.
    pub(super) fn migrate_waiting(&self) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let rows = tx
            .prepare("SELECT payload FROM task_runs WHERE state = 'waiting'")?
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let now = tx.query_row(
            "SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER)",
            [],
            |row| row.get(0),
        )?;
        for data in rows {
            let mut run: TaskRun = serde_json::from_str(&data)?;
            run.state = "result_available".into();
            finish(&tx, &mut run, now)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn owns_prompt(&self, prompt_id: &str) -> Result<bool> {
        Ok(self.lock()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_runs WHERE id = ?1) OR EXISTS(SELECT 1 FROM task_wakeups WHERE prompt_id = ?1)",
            [prompt_id], |row| row.get(0))?)
    }
}
