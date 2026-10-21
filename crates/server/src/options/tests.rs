use super::*;

fn options(arguments: &[&str], environment: &[(&str, &str)]) -> Result<Options> {
    parse_from(arguments.iter().map(|value| (*value).to_owned()), |key| {
        environment
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| OsString::from(value))
    })?
    .context("expected server options")
}

#[test]
fn session_limits_default_independently_and_accept_custom_seconds() {
    assert_eq!(
        options(&[], &[]).unwrap().session_timeouts,
        SessionTimeouts::from_seconds(43200, 1800).unwrap()
    );
    for (environment, expected) in [
        (
            vec![("AOW_AUTH_SESSION_MAX_AGE_SECONDS", "86400")],
            (86400, 1800),
        ),
        (
            vec![("AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS", "7200")],
            (43200, 7200),
        ),
        (
            vec![
                ("AOW_AUTH_SESSION_MAX_AGE_SECONDS", "60"),
                ("AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS", "120"),
            ],
            (60, 120),
        ),
    ] {
        assert_eq!(
            options(&[], &environment).unwrap().session_timeouts,
            SessionTimeouts::from_seconds(expected.0, expected.1).unwrap()
        );
    }
}

#[test]
fn command_line_session_limits_override_environment() {
    let options = options(
        &[
            "--session-max-age-seconds",
            "7200",
            "--session-idle-timeout-seconds",
            "600",
        ],
        &[
            ("AOW_AUTH_SESSION_MAX_AGE_SECONDS", "invalid"),
            ("AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS", "0"),
        ],
    )
    .unwrap();
    assert_eq!(
        options.session_timeouts,
        SessionTimeouts::from_seconds(7200, 600).unwrap()
    );
}

#[test]
fn invalid_or_missing_session_limits_fail_with_the_setting_name() {
    for (key, flag) in [
        (
            "AOW_AUTH_SESSION_MAX_AGE_SECONDS",
            "--session-max-age-seconds",
        ),
        (
            "AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS",
            "--session-idle-timeout-seconds",
        ),
    ] {
        for value in [
            "",
            "0",
            "-1",
            "1.5",
            "30m",
            "garbage",
            " 60",
            "18446744073709551616",
        ] {
            assert!(
                options(&[], &[(key, value)])
                    .unwrap_err()
                    .to_string()
                    .contains(key)
            );
            assert!(
                options(&[flag, value], &[])
                    .unwrap_err()
                    .to_string()
                    .contains(flag)
            );
        }
        assert!(
            options(&[flag], &[])
                .unwrap_err()
                .to_string()
                .contains("requires a value")
        );
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let error = parse_from(std::iter::empty(), |name| {
                (name == key).then(|| OsString::from_vec(vec![0xff]))
            })
            .unwrap_err();
            assert!(error.to_string().contains(key));
        }
    }
    assert!(SessionTimeouts::from_seconds(0, 1).is_err());
    assert!(SessionTimeouts::from_seconds(1, 0).is_err());
}
