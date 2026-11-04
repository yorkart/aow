use super::*;

// Invoked by the opt-in native log test with a unique marker; no service or
// global log configuration is modified by the fixture.
#[test]
#[ignore = "native subprocess fixture; see scripts/tests/unified-logging.test.mjs"]
fn native_log_fixture() {
    let Ok(marker) = std::env::var("AOW_LOG_TEST_MARKER") else {
        return;
    };
    assert!(init_from_env(c"test", "info"));
    let span = tracing::info_span!("fixture", run = %marker);
    let _entered = span.enter();
    tracing::info!("info marker {marker}");
    tracing::warn!("warn marker {marker}");
    tracing::error!("error marker {marker}");
    tracing::info!("UTF-8 中文 🦀 NUL:\0 END {marker}");
    report_error(&format!(
        "long {marker} {} tail {marker}",
        "诊断🦀".repeat(400)
    ));
    report_error(&format!("fatal marker {marker}"));
    let _ = std::panic::catch_unwind(|| panic!("panic marker {marker}"));
}
