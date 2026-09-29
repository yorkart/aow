use std::{collections::HashSet, sync::Arc, thread};

use super::*;

#[test]
fn matches_pinned_go_generator_and_encoding_vectors() {
    let mut generator = Generator::new(0).unwrap();
    for line in include_str!("../../tests/fixtures/upstream.tsv").lines() {
        if line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let timestamp = fields[0].parse().unwrap();
        let node_id = fields[1].parse().unwrap();
        if node_id != generator.node_id() {
            generator = Generator::new(node_id).unwrap();
        }
        let id = generator.generate_with(|| Ok(timestamp)).unwrap();
        assert_eq!(id.as_u64().to_string(), fields[2]);
        assert_eq!(id.to_base58(), fields[3]);
        assert_eq!(id.to_base62(), fields[4]);
        assert_eq!(id.timestamp_millis(), timestamp);
        assert_eq!(id.node_id(), node_id);
    }
}

#[test]
fn consumes_all_sequences_then_waits_for_the_next_millisecond() {
    let generator = Generator::new(1023).unwrap();
    for sequence in 0..=MAX_SEQUENCE {
        let id = generator.generate_with(|| Ok(1000)).unwrap();
        assert_eq!(id.sequence(), sequence);
        assert_eq!(id.timestamp_millis(), 1000);
    }
    let mut times = [1000, 1000, 1001].into_iter();
    let id = generator
        .generate_with(|| Ok(times.next().expect("generator should stop waiting")))
        .unwrap();
    assert_eq!(id.timestamp_millis(), 1001);
    assert_eq!(id.sequence(), 0);
    assert_eq!(times.next(), None);
}

#[test]
fn clock_rollback_is_rejected_without_reusing_a_sequence() {
    let generator = Generator::new(0).unwrap();
    let first = generator.generate_with(|| Ok(50)).unwrap();
    assert_eq!(
        generator.generate_with(|| Ok(49)),
        Err(Error::ClockMovedBackwards {
            previous: 50,
            current: 49,
        })
    );
    let resumed = generator.generate_with(|| Ok(50)).unwrap();
    assert_eq!(resumed.as_u64(), first.as_u64() + 1);
}

#[test]
fn timestamp_exhaustion_returns_an_error_instead_of_wrapping() {
    let generator = Generator::new(1023).unwrap();
    for _ in 0..=MAX_SEQUENCE {
        generator.generate_with(|| Ok(MAX_TIMESTAMP)).unwrap();
    }
    let mut times = [MAX_TIMESTAMP, MAX_TIMESTAMP + 1].into_iter();
    assert_eq!(
        generator.generate_with(|| Ok(times.next().unwrap())),
        Err(Error::TimestampOverflow)
    );
    let state = generator.state.lock().unwrap();
    assert_eq!(state.timestamp, MAX_TIMESTAMP);
    assert_eq!(state.sequence, MAX_SEQUENCE);
}

#[test]
fn validates_node_and_epoch_before_generating() {
    assert!(matches!(
        Generator::new(1024),
        Err(Error::InvalidNode(1024))
    ));
    assert!(matches!(
        Generator::with_epoch(0, u64::MAX),
        Err(Error::EpochInFuture)
    ));
    let generator = Generator::with_epoch(42, TWITTER_EPOCH_MILLIS + 1).unwrap();
    assert_eq!(generator.node_id(), 42);
    assert_eq!(generator.epoch_millis(), TWITTER_EPOCH_MILLIS + 1);
}

#[test]
fn new_generators_in_later_windows_do_not_need_persisted_sequences() {
    let before = Generator::new(1).unwrap();
    let after = Generator::new(1).unwrap();
    let old_id = before.generate_with(|| Ok(100)).unwrap();
    let new_id = after.generate_with(|| Ok(101)).unwrap();
    assert!(new_id > old_id);
    assert_eq!(new_id.sequence(), 0);
}

#[test]
fn distinct_nodes_separate_ids_in_the_same_millisecond() {
    let mut ids = HashSet::new();
    for node in 0..=MAX_NODE {
        let generator = Generator::new(node).unwrap();
        assert!(ids.insert(generator.generate_with(|| Ok(100)).unwrap()));
    }
}

#[test]
fn shared_generator_is_unique_and_ordered_under_concurrency() {
    let generator = Arc::new(Generator::new(19).unwrap());
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let generator = Arc::clone(&generator);
            thread::spawn(move || {
                let ids: Vec<_> = (0..5000).map(|_| generator.generate().unwrap()).collect();
                assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
                assert!(ids.iter().all(|id| id.node_id() == 19));
                ids
            })
        })
        .collect();
    let ids: Vec<_> = threads
        .into_iter()
        .flat_map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(ids.len(), 40_000);
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
}

#[test]
fn a_poisoned_generator_fails_closed() {
    let generator = Generator::new(1).unwrap();
    let result = std::panic::catch_unwind(|| {
        let _guard = generator.state.lock().unwrap();
        panic!("simulate a failed update");
    });
    assert!(result.is_err());
    assert_eq!(generator.generate(), Err(Error::Poisoned));
}
