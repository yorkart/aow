use super::*;

#[test]
fn maximum_value_matches_the_go_base58_vector() {
    let line = include_str!("../../tests/fixtures/upstream.tsv")
        .lines()
        .find(|line| line.starts_with("# max_id "))
        .unwrap();
    let fields: Vec<_> = line.split_whitespace().collect();
    let id = Snowflake::from_u64(i64::MAX as u64).unwrap();
    assert_eq!(id.as_u64().to_string(), fields[2]);
    assert_eq!(id.to_base58(), fields[3]);
    assert_eq!(id.to_base62(), fields[4]);
    assert_eq!(id.to_base58().len(), 11);
    assert_eq!(id.to_base62().len(), 11);
    assert_eq!(id.timestamp_millis(), (1 << 41) - 1);
    assert_eq!(id.node_id(), 1023);
    assert_eq!(id.sequence(), 4095);
}

#[test]
fn encodings_round_trip_boundaries_and_distributed_values() {
    let mut values = vec![0, 1, i64::MAX as u64];
    for radix in [58_u64, 62] {
        let mut power = radix;
        while power <= i64::MAX as u64 {
            values.extend([power - 1, power, power + 1]);
            let Some(next) = power.checked_mul(radix) else {
                break;
            };
            power = next;
        }
    }
    // Deterministic coverage across the full 63-bit range, without a rand dependency.
    let mut sample = 1_u64;
    for _ in 0..10_000 {
        sample = sample
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        values.push(sample & i64::MAX as u64);
    }
    for value in values {
        let id = Snowflake::from_u64(value).unwrap();
        let base62 = id.to_base62();
        let base58 = id.to_base58();
        assert!(base62.len() <= 11);
        assert!(base58.len() <= 11);
        assert_eq!(Snowflake::from_base62(&base62), Ok(id));
        assert_eq!(Snowflake::from_base58(&base58), Ok(id));
        assert_eq!(id.to_string().parse::<Snowflake>(), Ok(id));
    }
}

#[test]
fn rejects_invalid_noncanonical_and_overflowing_ids() {
    for invalid in ["", "00", "01", "-1", "+1", " 1", "1\n", "é", "012345678901"] {
        assert_eq!(Snowflake::from_base62(invalid), Err(Error::InvalidEncoding));
    }
    for invalid in ["", "11", "12", "0", "O", "I", "l", "-1", "é"] {
        assert_eq!(Snowflake::from_base58(invalid), Err(Error::InvalidEncoding));
    }
    assert_eq!(
        Snowflake::from_base62("zzzzzzzzzzz"),
        Err(Error::IdOutOfRange)
    );
    assert_eq!(
        Snowflake::from_base58("ZZZZZZZZZZZ"),
        Err(Error::IdOutOfRange)
    );
    assert_eq!(Snowflake::from_u64(1 << 63), Err(Error::IdOutOfRange));
    assert_eq!(Snowflake::from_u64(u64::MAX), Err(Error::IdOutOfRange));
    assert_eq!(Snowflake::from_base62("0").unwrap().as_u64(), 0);
    assert_eq!(Snowflake::from_base58("1").unwrap().as_u64(), 0);
}

#[test]
fn base62_is_case_sensitive_and_sorting_uses_the_numeric_value() {
    assert_eq!(Snowflake::from_base62("A").unwrap().as_u64(), 10);
    assert_eq!(Snowflake::from_base62("a").unwrap().as_u64(), 36);
    let smaller = Snowflake::from_u64(61).unwrap();
    let larger = Snowflake::from_u64(62).unwrap();
    assert!(smaller < larger);
    assert!(smaller.to_string() > larger.to_string());
}
