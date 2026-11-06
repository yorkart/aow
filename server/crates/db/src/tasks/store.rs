use super::TaskStatus;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskResource {
    pub kind: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: String,
    pub project_id: String,
    pub prompt: String,
    pub status: TaskStatus,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub agent_id: String,
    pub agent_role_id: Option<String>,
    pub is_orchestrator: bool,
    pub source_task_id: Option<String>,
    pub source_thread_id: Option<String>,
    pub workspace_intent: Option<String>,
    pub target_task_id: Option<String>,
    pub target_revision: Option<String>,
    pub workspace: String,
    pub workspace_mode: String,
    pub slug: Option<String>,
    pub initial_thread_id: Option<String>,
    pub pinned: bool,
    pub archived: bool,
    pub dependencies: Vec<String>,
    pub resources: Vec<TaskResource>,
}

#[derive(Clone)]
pub struct TaskDb(Arc<Mutex<Connection>>);

impl TaskDb {
    pub fn open(path: Option<&Path>) -> Result<Self> {
        let mut connection = aow_sqlite::open(path)?;
        aow_sqlite::migrate(
            &mut connection,
            "AowTasks",
            &[
                include_str!("sql/initialize.sql"),
                include_str!("sql/roles.sql"),
                include_str!("sql/routing.sql"),
                include_str!("sql/collaboration.sql"),
                include_str!("sql/executions.sql"),
                include_str!("sql/messages.sql"),
                include_str!("sql/properties.sql"),
                include_str!("sql/orchestrator.sql"),
            ],
        )?;
        let db = Self(Arc::new(Mutex::new(connection)));
        Ok(db)
    }

    pub(super) fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| anyhow::anyhow!("Task database lock poisoned"))
    }

    pub fn list(&self) -> Result<Vec<TaskRecord>> {
        let connection = self.lock()?;
        let mut query = connection.prepare("SELECT id, project_id, prompt, agent_id, workspace, initial_thread_id, pinned, archived, workspace_mode, slug, agent_role_id, is_orchestrator, source_task_id, source_thread_id, workspace_intent, target_task_id, target_revision, status, created_at, updated_at, completed_at FROM tasks ORDER BY rowid")?;
        let mut tasks = query
            .query_map([], |row| {
                Ok(TaskRecord {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    prompt: row.get(2)?,
                    agent_id: row.get(3)?,
                    workspace: row.get(4)?,
                    initial_thread_id: row.get(5)?,
                    pinned: row.get(6)?,
                    archived: row.get(7)?,
                    workspace_mode: row.get(8)?,
                    slug: row.get(9)?,
                    agent_role_id: row.get(10)?,
                    is_orchestrator: row.get(11)?,
                    source_task_id: row.get(12)?,
                    source_thread_id: row.get(13)?,
                    workspace_intent: row.get(14)?,
                    target_task_id: row.get(15)?,
                    target_revision: row.get(16)?,
                    status: row.get(17)?,
                    created_at: row.get(18)?,
                    updated_at: row.get(19)?,
                    completed_at: row.get(20)?,
                    dependencies: vec![],
                    resources: vec![],
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for task in &mut tasks {
            task.dependencies = connection
                .prepare(
                    "SELECT dependency_id FROM task_dependencies WHERE task_id = ?1 ORDER BY rowid",
                )?
                .query_map([&task.id], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            task.resources = connection.prepare("SELECT kind, resource_id FROM task_resources WHERE task_id = ?1 ORDER BY rowid")?
                .query_map([&task.id], |row| Ok(TaskResource { kind: row.get(0)?, id: row.get(1)? }))?.collect::<rusqlite::Result<_>>()?;
        }
        Ok(tasks)
    }

    pub fn insert(&self, task: &TaskRecord) -> Result<()> {
        self.insert_with_message(task, None)
    }

    pub fn insert_with_message(
        &self,
        task: &TaskRecord,
        message: Option<&super::MessageDelivery>,
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        tx.execute("INSERT INTO tasks (id, project_id, prompt, agent_id, workspace, initial_thread_id, pinned, archived, workspace_mode, slug, agent_role_id, is_orchestrator, source_task_id, source_thread_id, workspace_intent, target_task_id, target_revision, status, created_at, updated_at, completed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![task.id, task.project_id, task.prompt, task.agent_id, task.workspace, task.initial_thread_id, task.pinned, task.archived, task.workspace_mode, task.slug, task.agent_role_id, task.is_orchestrator, task.source_task_id, task.source_thread_id, task.workspace_intent, task.target_task_id, task.target_revision, task.status, task.created_at, task.updated_at, task.completed_at])?;
        replace_dependencies(&tx, &task.id, &task.dependencies)?;
        for resource in &task.resources {
            tx.execute(
                "INSERT INTO task_resources VALUES (?1, ?2, ?3)",
                params![task.id, resource.kind, resource.id],
            )?;
        }
        if let Some(message) = message {
            super::messages::insert(&tx, message)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn update(
        &self,
        id: &str,
        prompt: Option<&str>,
        pinned: Option<bool>,
        archived: Option<bool>,
        dependencies: Option<&[String]>,
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        ensure!(tx.execute("UPDATE tasks SET prompt = COALESCE(?2, prompt), pinned = COALESCE(?3, pinned), archived = COALESCE(?4, archived), updated_at = ?5 WHERE id = ?1", params![id, prompt, pinned, archived, timestamp()?])? == 1, "Task not found");
        if let Some(dependencies) = dependencies {
            replace_dependencies(&tx, id, dependencies)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        ensure!(
            self.lock()?.execute(
                "UPDATE tasks SET status = ?2,
                    updated_at = CASE WHEN status != ?2 THEN ?3 ELSE updated_at END,
                    completed_at = CASE WHEN ?2 = 'done' THEN CASE WHEN status = 'done' THEN completed_at ELSE ?3 END ELSE NULL END
                 WHERE id = ?1 AND archived = 0 AND is_orchestrator = 0",
                params![id, status, timestamp()?]
            )? == 1,
            "Active execution task not found"
        );
        Ok(())
    }

    pub fn status(&self, id: &str) -> Result<Option<TaskStatus>> {
        Ok(self
            .lock()?
            .query_row("SELECT status FROM tasks WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn attach(&self, id: &str, resource: &TaskResource) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND archived = 0)",
            [id],
            |row| row.get(0),
        )?;
        ensure!(active, "Active task not found");
        let owner: Option<String> = tx
            .query_row(
                "SELECT task_id FROM task_resources WHERE kind = ?1 AND resource_id = ?2",
                params![resource.kind, resource.id],
                |row| row.get(0),
            )
            .optional()?;
        ensure!(
            owner.as_deref().is_none_or(|owner| owner == id),
            "Resource already belongs to another task"
        );
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO task_resources VALUES (?1, ?2, ?3)",
            params![id, resource.kind, resource.id],
        )?;
        if inserted > 0 {
            tx.execute(
                "UPDATE tasks SET updated_at = ?2 WHERE id = ?1",
                params![id, timestamp()?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Removing organization never destroys agent sessions, terminals or worktrees.
    pub fn remove(&self, id: &str) -> Result<()> {
        ensure!(
            self.lock()?
                .execute("DELETE FROM tasks WHERE id = ?1", [id])?
                == 1,
            "Task not found"
        );
        Ok(())
    }
}

fn timestamp() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}

fn replace_dependencies(connection: &Connection, id: &str, dependencies: &[String]) -> Result<()> {
    connection.execute("DELETE FROM task_dependencies WHERE task_id = ?1", [id])?;
    for dependency in dependencies {
        ensure!(id != dependency, "A task cannot depend on itself");
        let valid: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM tasks AS a JOIN tasks AS b ON a.project_id = b.project_id WHERE a.id = ?1 AND b.id = ?2)", params![id, dependency], |row| row.get(0))?;
        ensure!(valid, "Dependencies must exist in the same project");
        let cycle: bool = connection.query_row("WITH RECURSIVE reachable(id) AS (SELECT ?2 UNION SELECT dependency_id FROM task_dependencies JOIN reachable ON task_id = reachable.id) SELECT EXISTS(SELECT 1 FROM reachable WHERE id = ?1)", params![id, dependency], |row| row.get(0))?;
        ensure!(!cycle, "Task dependencies cannot contain a cycle");
        connection.execute(
            "INSERT INTO task_dependencies VALUES (?1, ?2)",
            params![id, dependency],
        )?;
    }
    Ok(())
}
