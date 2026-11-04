use super::*;

#[test]
fn advances_from_the_fixed_anchor_without_resampling_wall_time() {
    let clock = Clock {
        base: Duration::from_micros(100_900),
        started: Instant::now(),
    };
    // The elapsed calculation only receives monotonic duration, so subsequent
    // wall-clock corrections cannot affect it. Preserve the fractional anchor.
    assert_eq!(clock.elapsed_with(Duration::ZERO), Ok(100));
    assert_eq!(clock.elapsed_with(Duration::from_micros(99)), Ok(100));
    assert_eq!(clock.elapsed_with(Duration::from_micros(100)), Ok(101));
    assert_eq!(clock.elapsed_with(Duration::from_secs(1)), Ok(1100));
}

#[test]
fn anchors_to_the_configured_epoch() {
    let epoch = crate::TWITTER_EPOCH_MILLIS;
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let clock = Clock::new(epoch).unwrap();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let anchor = clock.elapsed_with(Duration::ZERO).unwrap() as u128 + u128::from(epoch);
    assert!((before..=after).contains(&anchor));
    assert!(clock.elapsed_millis().unwrap() >= clock.elapsed_with(Duration::ZERO).unwrap());
}

#[test]
fn duration_overflow_is_reported() {
    let clock = Clock {
        base: Duration::MAX,
        started: Instant::now(),
    };
    assert_eq!(
        clock.elapsed_with(Duration::from_nanos(1)),
        Err(Error::TimestampOverflow)
    );
}
