//! Queries spanning sessions and messages along Hermes' native compression lineage.
use rusqlite::{Connection, Result, params};

use super::{CompactedMessageRecord, MessageRecord, messages, schema::has_column};

// Match Hermes' get_compression_tip: explicit branches/delegates are separate
// conversations, and a live continuation takes precedence over stale siblings.
pub(super) fn compression_edges(connection: &Connection) -> String {
    let origin = if has_column(connection, "sessions", "model_config") {
        let config = include_str!("sql/child_model_config.sql");
        format!(
            include_str!("sql/compression_origin_filter.sql"),
            config = config,
        )
    } else {
        String::new()
    };
    format!(include_str!("sql/compression_edges.sql"), origin = origin)
}

fn history(connection: &Connection) -> String {
    format!(
        include_str!("sql/history.sql"),
        edges = compression_edges(connection),
    )
}

pub(super) fn last_message_timestamp(connection: &Connection, id: &str) -> Result<Option<f64>> {
    connection.query_row(
        &format!(
            include_str!("sql/last_message_timestamp.sql"),
            history = history(connection),
        ),
        [id],
        |row| row.get(0),
    )
}

pub(super) fn compacted_messages(
    connection: &Connection,
    id: &str,
) -> Result<Option<Vec<CompactedMessageRecord>>> {
    if !has_column(connection, "messages", "compacted") {
        return Ok(None);
    }
    let history = history(connection);
    let compacted: bool = connection.query_row(
        &format!(include_str!("sql/has_compaction.sql"), history),
        [id],
        |row| row.get(0),
    )?;
    if !compacted {
        return Ok(None);
    }
    let sql = format!(include_str!("sql/messages.sql"), history);
    let rows = connection
        .prepare(&sql)?
        .query_map([id], messages::read_compacted)?
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(rows))
}

pub(super) fn recent_messages(
    connection: &Connection,
    id: &str,
    preceding_user_turns: usize,
) -> Result<Vec<MessageRecord>> {
    let sql = format!(
        include_str!("sql/snapshot_records.sql"),
        history = history(connection),
        active = messages::active_filter(connection),
    );
    connection
        .prepare(&sql)?
        .query_map(params![id, preceding_user_turns], messages::read)?
        .collect()
}

pub(super) fn tail_messages(
    connection: &Connection,
    id: &str,
    cursor: i64,
    limit: usize,
) -> Result<Vec<MessageRecord>> {
    let sql = format!(
        include_str!("sql/tail_records.sql"),
        history = history(connection),
        active = messages::active_filter(connection),
    );
    connection
        .prepare(&sql)?
        .query_map(params![id, cursor, limit], messages::read)?
        .collect()
}
