use super::*;
use rusqlite::{Connection, params};
const RETAIN: u64 = 4096;
pub(super) fn open(root: &std::path::Path) -> Result<Connection> {
    let dir = root.join("data/host-daemon");
    std::fs::create_dir_all(&dir)?;
    let db = Connection::open(dir.join("completion-events.sqlite"))?;
    db.busy_timeout(std::time::Duration::from_secs(5))?;
    db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS completions (sequence INTEGER PRIMARY KEY AUTOINCREMENT, payload TEXT NOT NULL);")?;
    Ok(db)
}
pub(super) fn bounds(db: &Connection) -> Result<(u64, u64)> {
    Ok(db.query_row(
        "SELECT COALESCE(MIN(sequence),0),COALESCE(MAX(sequence),0) FROM completions",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}
pub(super) fn append(db: &mut Connection, events: &[Completion]) -> Result<u64> {
    let tx = db.transaction()?;
    for event in events {
        tx.execute(
            "INSERT INTO completions(payload) VALUES (?)",
            [serde_json::to_string(event)?],
        )?;
    }
    let (_, head) = bounds(&tx)?;
    tx.execute(
        "DELETE FROM completions WHERE sequence <= ?",
        [head.saturating_sub(RETAIN)],
    )?;
    tx.commit()?;
    Ok(head)
}
pub(super) fn read(db: &Connection, after: u64) -> Result<Vec<serde_json::Value>> {
    let (oldest, head) = bounds(db)?;
    anyhow::ensure!(after <= head, "Completion cursor is ahead of node log");
    anyhow::ensure!(
        oldest == 0 || after >= oldest.saturating_sub(1),
        "Completion cursor expired (oldest: {oldest}, head: {head})"
    );
    let mut query = db.prepare(
        "SELECT sequence,payload FROM completions WHERE sequence > ? ORDER BY sequence LIMIT 128",
    )?;
    let rows = query.query_map(params![after], |row| {
        Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.map(|row| {let (sequence,payload)=row?; Ok(serde_json::json!({"type":"completion","sequence":sequence,"event":serde_json::from_str::<serde_json::Value>(&payload)?}))}).collect()
}
