use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRecord {
    pub workspace_mode: String,
    pub slug: Option<String>,
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub prompt: String,
    pub agent_runtime_id: String,
    pub agent_role_id: Option<String>,
    pub session_id: Option<String>,
    pub workspace: String,
    pub thread_id: String,
    pub pinned: bool,
    pub archived: bool,
}

#[derive(Clone)]
pub struct TaskDb(Arc<Mutex<Connection>>);

impl TaskDb {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let mut connection = crate::sqlite::open(path)?;
        crate::sqlite::migrate(
            &mut connection,
            "AowTasks",
            &[
                include_str!("sql/initialize.sql"),
                include_str!("sql/002_workspace_intent.sql"),
                include_str!("sql/003_session_identity.sql"),
            ],
        )?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    pub fn list(&self) -> Result<Vec<TaskRecord>> {
        let connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Task database lock poisoned"))?;
        let mut query = connection.prepare("SELECT id, project_id, parent_id, prompt, agent_runtime_id, workspace, thread_id, pinned, archived, workspace_mode, slug, agent_role_id, session_id FROM tasks ORDER BY rowid")?;
        Ok(query
            .query_map([], |row| {
                Ok(TaskRecord {
                    workspace_mode: row.get(9)?,
                    slug: row.get(10)?,
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    parent_id: row.get(2)?,
                    prompt: row.get(3)?,
                    agent_runtime_id: row.get(4)?,
                    agent_role_id: row.get(11)?,
                    session_id: row.get(12)?,
                    workspace: row.get(5)?,
                    thread_id: row.get(6)?,
                    pinned: row.get(7)?,
                    archived: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    pub fn insert(&self, task: &TaskRecord) -> Result<()> {
        let mut connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Task database lock poisoned"))?;
        let transaction = connection.transaction()?;
        if let Some(parent) = &task.parent_id {
            let valid: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND project_id = ?2 AND archived = 0)", params![parent, task.project_id], |row| row.get(0))?;
            ensure!(
                valid,
                "Parent task must be active and belong to the same project"
            );
        }
        transaction.execute("INSERT INTO tasks (id, project_id, parent_id, prompt, agent_runtime_id, workspace, thread_id, workspace_mode, slug, agent_role_id, session_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)", params![task.id, task.project_id, task.parent_id, task.prompt, task.agent_runtime_id, task.workspace, task.thread_id, task.workspace_mode, task.slug, task.agent_role_id, task.session_id])?;
        transaction.commit()?;
        Ok(())
    }

    /// Ignore ordinary ACP threads; a Task binding is owned by its stable thread and runtime.
    pub fn bind_session(&self, thread_id: &str, runtime_id: &str, session_id: &str) -> Result<()> {
        ensure!(!session_id.trim().is_empty(), "Native session ID is empty");
        let connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Task database lock poisoned"))?;
        let owner: Option<String> = connection
            .query_row(
                "SELECT agent_runtime_id FROM tasks WHERE thread_id = ?1 AND archived = 0",
                [thread_id],
                |row| row.get(0),
            )
            .optional()?;
        ensure!(
            owner.as_deref().is_none_or(|runtime| runtime == runtime_id),
            "Task runtime does not match its ACP session"
        );
        connection.execute("UPDATE tasks SET session_id = ?3 WHERE thread_id = ?1 AND agent_runtime_id = ?2 AND archived = 0 AND (session_id IS NULL OR session_id != ?3)", params![thread_id, runtime_id, session_id])?;
        Ok(())
    }

    pub fn pin(&self, id: &str, pinned: bool) -> Result<()> {
        let connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Task database lock poisoned"))?;
        ensure!(
            connection.execute(
                "UPDATE tasks SET pinned = ?2 WHERE id = ?1 AND archived = 0",
                params![id, pinned]
            )? == 1,
            "Active task not found"
        );
        Ok(())
    }

    /// Keep ownership until cleanup succeeds. Retrying a partial cleanup remains possible.
    pub fn finish(&self, id: &str, archive: bool) -> Result<()> {
        let mut connection = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Task database lock poisoned"))?;
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
            [id],
            |row| row.get(0),
        )?;
        ensure!(exists, "Task not found");
        let descendants = "WITH RECURSIVE descendants(id) AS (SELECT id FROM tasks WHERE id = ?1 UNION ALL SELECT tasks.id FROM tasks JOIN descendants ON tasks.parent_id = descendants.id)";
        if archive {
            transaction.execute(&format!("{descendants} UPDATE tasks SET archived = 1, pinned = 0 WHERE id IN (SELECT id FROM descendants)"), [id])?;
        } else {
            // One statement removes the entire subtree before foreign-key checking.
            transaction.execute(
                &format!(
                    "{descendants} DELETE FROM tasks WHERE id IN (SELECT id FROM descendants)"
                ),
                [id],
            )?;
        }
        transaction.commit().context("Cannot finish task")?;
        Ok(())
    }
}
