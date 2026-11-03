use super::{TaskDb, TaskRun, WakeupDelivery, collaboration, deliveries};
use anyhow::{Result, ensure};
use rusqlite::params;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct InboxEvent {
    pub id: String,
    pub run: TaskRun,
    pub read: bool,
    pub archived: bool,
    pub created_at: i64,
    pub delivery: Option<WakeupDelivery>,
}

#[derive(Serialize)]
pub struct InboxPage {
    pub items: Vec<InboxEvent>,
    pub unread: u64,
    pub has_more: bool,
}

impl TaskDb {
    pub fn inbox(
        &self,
        project: Option<&str>,
        unread: bool,
        archived: bool,
        offset: u32,
        limit: u32,
    ) -> Result<InboxPage> {
        let connection = self.lock()?;
        let count: u64 = connection.query_row(
            "SELECT COUNT(*) FROM task_inbox WHERE is_read = 0 AND archived = 0 AND (?1 IS NULL OR project_id = ?1)",
            [project], |row| row.get(0))?;
        let mut query = connection.prepare(
            "SELECT id, run_id, is_read, archived, created_at FROM task_inbox
             WHERE (?1 IS NULL OR project_id = ?1) AND (?2 = 0 OR is_read = 0) AND archived = ?3
             ORDER BY created_at DESC, id DESC LIMIT ?4 OFFSET ?5",
        )?;
        let rows = query
            .query_map(
                params![project, unread, archived, limit + 1, offset],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = rows.len() > limit as usize;
        let items = rows
            .into_iter()
            .take(limit as usize)
            .map(|(id, run_id, read, archived, created_at)| {
                Ok(InboxEvent {
                    run: collaboration::run(&connection, &run_id)?
                        .ok_or_else(|| anyhow::anyhow!("Missing event result"))?,
                    delivery: deliveries::delivery(&connection, &id)?,
                    id,
                    read,
                    archived,
                    created_at,
                })
            })
            .collect::<Result<_>>()?;
        Ok(InboxPage {
            items,
            unread: count,
            has_more,
        })
    }

    pub fn update_inbox(&self, id: &str, read: Option<bool>, archived: Option<bool>) -> Result<()> {
        ensure!(self.lock()?.execute("UPDATE task_inbox SET is_read = COALESCE(?2, is_read), archived = COALESCE(?3, archived) WHERE id = ?1",
            params![id, read, archived])? == 1, "Inbox event not found");
        Ok(())
    }
}
