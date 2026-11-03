use super::{MessageDelivery, TaskDb, TaskEvent, messages};
use anyhow::{Result, ensure};
use rusqlite::params;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct InboxEvent {
    #[serde(flatten)]
    pub event: TaskEvent,
    pub read: bool,
    pub archived: bool,
    pub delivery: Option<MessageDelivery>,
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
        let count: u64 = connection.query_row("SELECT COUNT(*) FROM task_events WHERE is_read=0 AND archived=0 AND (?1 IS NULL OR project_id=?1)", [project], |row| row.get(0))?;
        let mut query = connection.prepare("SELECT payload,is_read,archived FROM task_events WHERE (?1 IS NULL OR project_id=?1) AND (?2=0 OR is_read=0) AND archived=?3 ORDER BY created_at DESC,id DESC LIMIT ?4 OFFSET ?5")?;
        let rows = query
            .query_map(
                params![project, unread, archived, limit + 1, offset],
                |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let has_more = rows.len() > limit as usize;
        let items = rows
            .into_iter()
            .take(limit as usize)
            .map(|(data, read, archived)| {
                let event: TaskEvent = serde_json::from_str(&data)?;
                Ok(InboxEvent {
                    delivery: messages::for_event(&connection, &event.id)?,
                    event,
                    read,
                    archived,
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
        ensure!(self.lock()?.execute("UPDATE task_events SET is_read=COALESCE(?2,is_read), archived=COALESCE(?3,archived) WHERE id=?1", params![id,read,archived])? == 1, "Inbox event not found");
        Ok(())
    }
}
