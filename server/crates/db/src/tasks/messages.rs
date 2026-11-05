//! Durable message admission. ACP owns execution outcomes and conversation history.
use super::TaskDb;
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSource {
    pub task_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
    pub request: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Delivered,
    Blocked,
}
impl DeliveryState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Delivered => "delivered",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDelivery {
    pub id: String,
    pub request_id: String,
    pub task_id: String,
    pub thread_id: String,
    pub prompt: String,
    pub source: Option<MessageSource>,
    pub event_id: Option<String>,
    pub state: DeliveryState,
    pub created_at: i64,
    pub attempts: u32,
    pub retry_at: i64,
    pub error: Option<String>,
}

pub(super) fn insert(connection: &Connection, message: &MessageDelivery) -> Result<()> {
    connection.execute("INSERT INTO task_messages (id, task_id, request_id, thread_id, event_id, state, payload) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![message.id,message.task_id,message.request_id,message.thread_id,message.event_id,message.state.as_str(),serde_json::to_string(message)?])?;
    Ok(())
}

fn decode(data: Option<String>) -> Result<Option<MessageDelivery>> {
    data.map(|data| serde_json::from_str(&data).map_err(Into::into))
        .transpose()
}

pub(super) fn for_event(connection: &Connection, event: &str) -> Result<Option<MessageDelivery>> {
    decode(
        connection
            .query_row(
                "SELECT payload FROM task_messages WHERE event_id = ?1",
                [event],
                |row| row.get(0),
            )
            .optional()?,
    )
}

impl TaskDb {
    /// Correlate an ACP admission receipt without adding routing data to the prompt.
    pub fn message_for_prompt(
        &self,
        task: &str,
        thread: &str,
        id: &str,
    ) -> Result<Option<MessageDelivery>> {
        decode(
            self.lock()?
                .query_row(
                    "SELECT payload FROM task_messages WHERE id=?1 AND task_id=?2 AND thread_id=?3",
                    params![id, task, thread],
                    |row| row.get(0),
                )
                .optional()?,
        )
    }

    pub fn message_for_request(
        &self,
        task: &str,
        request: &str,
    ) -> Result<Option<MessageDelivery>> {
        decode(
            self.lock()?
                .query_row(
                    "SELECT payload FROM task_messages WHERE task_id = ?1 AND request_id = ?2",
                    params![task, request],
                    |row| row.get(0),
                )
                .optional()?,
        )
    }

    pub fn enqueue_message(&self, message: &MessageDelivery) -> Result<MessageDelivery> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let existing = decode(
            tx.query_row(
                "SELECT payload FROM task_messages WHERE task_id = ?1 AND request_id = ?2",
                params![message.task_id, message.request_id],
                |row| row.get(0),
            )
            .optional()?,
        )?;
        if let Some(existing) = existing {
            ensure!(
                existing.thread_id == message.thread_id
                    && existing.prompt == message.prompt
                    && existing.event_id == message.event_id
                    && existing.source.as_ref().map(|s| &s.thread_id)
                        == message.source.as_ref().map(|s| &s.thread_id),
                "Request ID already used for another message"
            );
            return Ok(existing);
        }
        insert(&tx, message)?;
        tx.commit()?;
        Ok(message.clone())
    }

    pub fn pending_messages(&self) -> Result<Vec<MessageDelivery>> {
        let connection = self.lock()?;
        let mut query = connection
            .prepare("SELECT payload FROM task_messages WHERE state = 'pending' ORDER BY rowid")?;
        query
            .query_map([], |row| row.get::<_, String>(0))?
            .map(|data| Ok(serde_json::from_str(&data?)?))
            .collect()
    }

    pub fn update_message(
        &self,
        id: &str,
        state: DeliveryState,
        error: Option<String>,
        retry_at: i64,
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut message = decode(
            tx.query_row(
                "SELECT payload FROM task_messages WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?,
        )?
        .ok_or_else(|| anyhow::anyhow!("Message not found"))?;
        if message.state != DeliveryState::Delivered {
            message.state = state;
            if error.is_some() {
                message.attempts += 1;
            }
            message.error = error;
            message.retry_at = retry_at;
            tx.execute(
                "UPDATE task_messages SET state=?2,payload=?3 WHERE id=?1",
                params![id, state.as_str(), serde_json::to_string(&message)?],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn retry_delivery(&self, event: &str) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut message =
            for_event(&tx, event)?.ok_or_else(|| anyhow::anyhow!("Delivery not found"))?;
        ensure!(
            message.state == DeliveryState::Blocked,
            "Only blocked deliveries can be retried"
        );
        // Retain identity: ACP may have accepted the message before the connection failed.
        message.state = DeliveryState::Pending;
        message.attempts = 0;
        message.retry_at = 0;
        message.error = None;
        tx.execute(
            "UPDATE task_messages SET state='pending',payload=?2 WHERE id=?1",
            params![message.id, serde_json::to_string(&message)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}
