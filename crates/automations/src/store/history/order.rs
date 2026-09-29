use std::cmp::Ordering;

/// Compare creation times embedded in either Snowflake or legacy timestamp IDs.
/// Keep this ordering shared by pagination and retention. The string tie-breaker
/// also preserves the ordering of legacy opaque IDs.
pub(super) fn compare_ids(left: &str, right: &str) -> Ordering {
    timestamp(left)
        .cmp(&timestamp(right))
        .then_with(|| left.cmp(right))
}

fn timestamp(id: &str) -> Option<u64> {
    if (12..=13).contains(&id.len())
        && let Ok(value) = id.parse::<aow_id::Snowflake>()
    {
        return Some(value.timestamp_millis() + aow_id::TWITTER_EPOCH_MILLIS);
    }
    let (time, _) = id.split_once('_')?;
    chrono::NaiveDateTime::parse_from_str(time, "%Y%m%dT%H%M%S%3fZ")
        .ok()?
        .and_utc()
        .timestamp_millis()
        .try_into()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_mixed_formats_by_time_instead_of_alphabet() {
        let millis = chrono::DateTime::parse_from_rfc3339("2026-09-29T12:00:00Z")
            .unwrap()
            .timestamp_millis() as u64;
        let id = aow_id::Snowflake::from_u64((millis - aow_id::TWITTER_EPOCH_MILLIS) << 22)
            .unwrap()
            .to_string();
        assert_eq!(timestamp(&id), Some(millis));
        assert_eq!(timestamp("20260929T120000000Z_1234"), Some(millis));
        assert!(compare_ids("20260928T120000000Z_9999", &id).is_lt());
        assert!(compare_ids(&id, "20260930T120000000Z_0000").is_lt());
        assert!(compare_ids("run-1", "run-2").is_lt());
    }
}
