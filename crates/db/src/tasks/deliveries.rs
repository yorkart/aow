use super::{TaskDb, collaboration};
use anyhow::{Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WakeupDelivery {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub continuation_run_id: Option<String>,
    pub prompt_id: String,
    pub state: String,
    pub attempts: u32,
    pub retry_at: i64,
    pub error: Option<String>,
}

pub(super) fn delivery(connection: &Connection, id: &str) -> Result<Option<WakeupDelivery>> {
    let data: Option<String> = connection
        .query_row(
            "SELECT payload FROM task_wakeups WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    data.map(|data| serde_json::from_str(&data).map_err(Into::into))
        .transpose()
}

fn save(connection: &Connection, delivery: &WakeupDelivery) -> Result<()> {
    connection.execute(
        "UPDATE task_wakeups SET prompt_id = ?2, state = ?3, payload = ?4 WHERE id = ?1",
        params![
            delivery.id,
            delivery.prompt_id,
            delivery.state,
            serde_json::to_string(delivery)?
        ],
    )?;
    Ok(())
}

impl TaskDb {
    pub fn active_deliveries(&self) -> Result<Vec<WakeupDelivery>> {
        let connection = self.lock()?;
        let mut query = connection.prepare("SELECT payload FROM task_wakeups WHERE state IN ('pending', 'admitted') ORDER BY rowid")?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter()
            .map(|data| serde_json::from_str(data).map_err(Into::into))
            .collect()
    }

    pub fn update_delivery(
        &self,
        id: &str,
        state: &str,
        error: Option<String>,
        retry_at: i64,
    ) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut delivery =
            delivery(&tx, id)?.ok_or_else(|| anyhow::anyhow!("Delivery not found"))?;
        if delivery.state != "processed" {
            delivery.state = state.into();
            if error.is_some() {
                delivery.attempts += 1;
            }
            delivery.error = error;
            delivery.retry_at = retry_at;
            save(&tx, &delivery)?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn process_delivery(&self, id: &str, conclusion: Option<String>, now: i64) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut delivery =
            delivery(&tx, id)?.ok_or_else(|| anyhow::anyhow!("Delivery not found"))?;
        if delivery.state != "processed" {
            delivery.state = "processed".into();
            delivery.error = None;
            save(&tx, &delivery)?;
            if let Some(parent) = &delivery.continuation_run_id
                && let Some(mut run) = collaboration::run(&tx, parent)?
                && !run.terminal()
            {
                run.state = "result_available".into();
                run.result = conclusion;
                run.error = None;
                collaboration::finish(&tx, &mut run, now)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// A retry starts a new delivery attempt, never reruns the completed child.
    /// The caller supplies a fresh ID only after an explicitly requested retry.
    pub fn retry_delivery(&self, id: &str, prompt_id: &str) -> Result<()> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut delivery =
            delivery(&tx, id)?.ok_or_else(|| anyhow::anyhow!("Delivery not found"))?;
        ensure!(
            delivery.state == "blocked",
            "Only blocked deliveries can be retried"
        );
        delivery.prompt_id = prompt_id.into();
        delivery.state = "pending".into();
        delivery.attempts = 0;
        delivery.retry_at = 0;
        delivery.error = None;
        save(&tx, &delivery)?;
        tx.commit()?;
        Ok(())
    }
}
