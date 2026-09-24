use super::websocket::*;
use super::*;

use futures_util::Sink;
use std::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

struct PendingSink {
    polled: Arc<Notify>,
}

impl Sink<Message> for PendingSink {
    type Error = std::convert::Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.polled.notify_one();
        Poll::Pending
    }

    fn start_send(self: Pin<&mut Self>, _item: Message) -> Result<(), Self::Error> {
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

struct PendingCloseSink {
    close_polls: Arc<AtomicUsize>,
}

impl Sink<Message> for PendingCloseSink {
    type Error = std::convert::Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, _item: Message) -> Result<(), Self::Error> {
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.close_polls.fetch_add(1, Ordering::Relaxed);
        Poll::Pending
    }
}

struct CollectingSink {
    messages: Vec<Message>,
    first_binary: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Sink<Message> for CollectingSink {
    type Error = std::convert::Infallible;

    fn poll_ready(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        if matches!(item, Message::Binary(_))
            && let Some(hook) = self.first_binary.take()
        {
            hook();
        }
        self.messages.push(item);
        Ok(())
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

fn test_runtime() -> (Arc<Runtime>, SpawnedRuntime) {
    let tracker = Arc::new(SpawnTracker::default());
    let spawned = spawn_runtime_blocking(
        format!("unit-test-{}", Uuid::new_v4()),
        TerminalRuntimeSpec {
            cwd: "/".to_owned(),
            shell: "/bin/sh".to_owned(),
            arguments: Vec::new(),
            environment: Default::default(),
            rows: 24,
            cols: 80,
        },
        tracker.begin(),
        None,
    )
    .expect("test runtime spawns");
    let runtime = spawned.runtime.clone();
    // Keep the spawned child and unread PTY reader in a guard instead of
    // starting background workers. Tests append output explicitly, so no
    // shell prompt or asynchronous status event can pollute stream
    // boundaries. Dropping the guard still kills and reaps the child.
    (runtime, spawned)
}

#[test]
fn validates_terminal_dimensions() {
    assert!(validate_dimensions(24, 80).is_ok());
    assert!(validate_dimensions(0, 80).is_err());
    assert!(validate_dimensions(24, 1001).is_err());
}

#[test]
fn runtime_environment_inherits_and_applies_additions_and_overrides() {
    let directory = tempfile::tempdir().unwrap();
    let output_path = directory.path().join("environment.txt");
    for override_path in [None, Some("/configured/bin")] {
        let mut environment = std::collections::BTreeMap::from([
            (
                "AOW_TEST_OUTPUT".to_owned(),
                output_path.to_string_lossy().into_owned(),
            ),
            (
                "AOW_TEST_ADDED".to_owned(),
                "value with spaces=equals".to_owned(),
            ),
            ("AOW_TEST_EMPTY".to_owned(), String::new()),
        ]);
        if let Some(path) = override_path {
            environment.insert("PATH".to_owned(), path.to_owned());
        }
        let tracker = Arc::new(SpawnTracker::default());
        let mut spawned = spawn_runtime_blocking(
            format!("environment-test-{}", Uuid::new_v4()),
            TerminalRuntimeSpec {
                cwd: directory.path().to_string_lossy().into_owned(),
                shell: "/bin/sh".to_owned(),
                arguments: vec!["-c".to_owned(),
                    "printf '%s\\n' \"$PATH\" \"$AOW_TEST_ADDED\" \"${AOW_TEST_EMPTY-unset}\" > \"$AOW_TEST_OUTPUT\"".to_owned()],
                environment,
                rows: 24,
                cols: 80,
            },
            tracker.begin(),
            None,
        ).unwrap();
        let status = spawned.child.as_mut().unwrap().wait().unwrap();
        assert_eq!(status.exit_code(), 0);
        let expected_path = override_path
            .map(str::to_owned)
            .unwrap_or_else(|| std::env::var("PATH").unwrap());
        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            format!("{expected_path}\nvalue with spaces=equals\n\n")
        );
        // The child was already reaped, so the guard only releases its handles.
        spawned.child.take();
    }
}

#[test]
fn session_stat_parser_assumption_matches_this_process() {
    #[cfg(target_os = "linux")]
    {
        let session = unsafe { libc::getsid(0) };
        assert!(linux_session_members(session).contains(&(std::process::id() as i32)));
    }
}

#[test]
fn scrollback_retains_exactly_the_last_eight_mebibytes() {
    assert_eq!(SCROLLBACK_LIMIT, 8 * 1024 * 1024);
    let bytes = (0..SCROLLBACK_LIMIT + 257)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let mut output = OutputState {
        scrollback: VecDeque::new(),
        base_offset: 0,
        next_offset: 0,
        closed: false,
        state_parser: vte::Parser::new(),
        terminal: TerminalState::default(),
    };
    assert_eq!(output.append(&bytes, SCROLLBACK_LIMIT).unwrap(), 0);

    assert_eq!(output.scrollback.len(), SCROLLBACK_LIMIT);
    assert_eq!(output.base_offset, 257);
    assert_eq!(output.next_offset, (SCROLLBACK_LIMIT + 257) as u64);
    assert_eq!(
        output.scrollback.make_contiguous(),
        &bytes[bytes.len() - SCROLLBACK_LIMIT..]
    );

    let exact = output.snapshot(Some(output.base_offset + 11));
    assert!(!exact.reset);
    assert_eq!(exact.offset, 268);
    assert_eq!(exact.replay, bytes[bytes.len() - SCROLLBACK_LIMIT + 11..]);

    let stale = output.snapshot(Some(output.base_offset - 1));
    assert!(stale.reset);
    assert_eq!(stale.offset, output.base_offset);
    assert_eq!(stale.replay, bytes[bytes.len() - SCROLLBACK_LIMIT..]);
    let replay_chunks = replay_chunks(&stale.replay).collect::<Vec<_>>();
    assert_eq!(
        replay_chunks.len(),
        SCROLLBACK_LIMIT.div_ceil(REPLAY_CHUNK_SIZE)
    );
    assert!(
        replay_chunks
            .iter()
            .all(|chunk| !chunk.is_empty() && chunk.len() <= REPLAY_CHUNK_SIZE)
    );
    assert_eq!(
        replay_chunks.iter().map(|chunk| chunk.len()).sum::<usize>(),
        SCROLLBACK_LIMIT
    );

    let future = output.snapshot(Some(output.next_offset + 1));
    assert!(future.reset);
    assert_eq!(future.offset, output.base_offset);
    assert_eq!(future.replay, bytes[bytes.len() - SCROLLBACK_LIMIT..]);
}

#[test]
fn fresh_reset_uses_valid_vt_snapshot_but_resume_stays_raw() {
    let mut output = OutputState {
        scrollback: VecDeque::from(b"0123456789".to_vec()),
        base_offset: 0,
        next_offset: 10,
        closed: false,
        state_parser: vte::Parser::new(),
        terminal: TerminalState::default(),
    };
    let snapshot = VtSnapshot {
        generation: "epoch".to_owned(),
        applied_offset: 6,
        cols: 120,
        rows: 40,
        ansi: "serialized-screen".to_owned(),
        lines: Vec::new(),
    };

    let fresh = output.snapshot_with_vt(None, Some(snapshot.clone()), "epoch");
    assert!(fresh.reset);
    assert_eq!(fresh.offset, 6);
    assert_eq!(fresh.replay, b"6789");
    assert_eq!(fresh.restore.as_deref(), Some("serialized-screen"));
    assert_eq!(fresh.restore_cols, Some(120));
    assert_eq!(fresh.restore_rows, Some(40));
    assert_eq!(
        fresh.restore_diagnostics,
        RestoreDiagnostics {
            snapshot_selected: true,
            reason: "selected",
            snapshot_bytes: "serialized-screen".len(),
            snapshot_offset: Some(6),
        }
    );

    let resumed = output.snapshot_with_vt(Some(7), Some(snapshot), "epoch");
    assert!(!resumed.reset);
    assert_eq!(resumed.offset, 7);
    assert_eq!(resumed.replay, b"789");
    assert_eq!(resumed.restore, None);
    assert_eq!(resumed.restore_cols, None);
    assert_eq!(resumed.restore_rows, None);
    assert_eq!(
        resumed.restore_diagnostics,
        RestoreDiagnostics {
            snapshot_selected: false,
            reason: "resume_cursor_used",
            snapshot_bytes: 0,
            snapshot_offset: None,
        }
    );
}

#[test]
fn invalid_vt_snapshot_falls_back_to_raw_replay() {
    let mut output = OutputState {
        scrollback: VecDeque::from(b"56789".to_vec()),
        base_offset: 5,
        next_offset: 10,
        closed: false,
        state_parser: vte::Parser::new(),
        terminal: TerminalState::default(),
    };
    for (snapshot, expected_reason) in [
        (
            VtSnapshot {
                generation: "old-epoch".to_owned(),
                applied_offset: 7,
                cols: 80,
                rows: 24,
                ansi: "wrong generation".to_owned(),
                lines: Vec::new(),
            },
            "generation_mismatch",
        ),
        (
            VtSnapshot {
                generation: "epoch".to_owned(),
                applied_offset: 4,
                cols: 80,
                rows: 24,
                ansi: "outside ring".to_owned(),
                lines: Vec::new(),
            },
            "stale_offset",
        ),
    ] {
        let fresh = output.snapshot_with_vt(None, Some(snapshot), "epoch");
        assert!(fresh.reset);
        assert_eq!(fresh.offset, 5);
        assert_eq!(fresh.replay, b"56789");
        assert_eq!(fresh.restore.as_deref(), Some(""));
        assert_eq!(fresh.restore_cols, None);
        assert_eq!(fresh.restore_rows, None);
        assert_eq!(fresh.restore_diagnostics.reason, expected_reason);
        assert!(!fresh.restore_diagnostics.snapshot_selected);
    }

    let unavailable = output.snapshot_with_vt(None, None, "epoch");
    assert_eq!(unavailable.restore_diagnostics.reason, "snapshot_not_ready");
    assert!(!unavailable.restore_diagnostics.snapshot_selected);
}

#[test]
fn osc_titles_cross_utf8_chunks_and_survive_scrollback_truncation() {
    let mut output = OutputState {
        scrollback: VecDeque::new(),
        base_offset: 0,
        next_offset: 0,
        closed: false,
        state_parser: vte::Parser::new(),
        terminal: TerminalState::default(),
    };
    let raw = "\x1b]0; 修复登录;保持会话 \x07".as_bytes();
    for chunk in raw
        .chunks(1)
        .chain([b"ordinary output beyond the small scrollback".as_slice()])
    {
        output.state_parser.advance(&mut output.terminal, chunk);
        output.append(chunk, 4).unwrap();
    }
    assert_eq!(output.terminal.title.as_deref(), Some("修复登录;保持会话"));
    assert!(output.base_offset > 0);
    assert!(!output.snapshot(None).replay.contains(&0x1b));
    output
        .state_parser
        .advance(&mut output.terminal, b"\x1b]2;renamed\x1b");
    output.state_parser.advance(&mut output.terminal, b"\\");
    assert_eq!(output.terminal.title.as_deref(), Some("renamed"));
    output.state_parser.advance(
        &mut output.terminal,
        b"\x1b]1;icon only\x07\x1b]52;c;aGVsbG8=\x07",
    );
    assert_eq!(output.terminal.title.as_deref(), Some("renamed"));
    output
        .state_parser
        .advance(&mut output.terminal, b"\x1b]0;\x07");
    assert_eq!(output.terminal.title, None);
}

#[test]
fn osc_titles_are_bounded_and_normalized_without_disturbing_modes() {
    let mut parser = vte::Parser::new();
    let mut terminal = TerminalState::default();
    parser.advance(
        &mut terminal,
        "\x1b]2;  标题\u{202e}\u{200b}  内容  \x07".as_bytes(),
    );
    assert_eq!(terminal.title.as_deref(), Some("标题 内容"));
    parser.advance(&mut terminal, b"\x1b]2;");
    for _ in 0..1024 {
        parser.advance(&mut terminal, &[b'a'; 1024]);
    }
    parser.advance(&mut terminal, b"\x07\x1b[?2004h");
    assert_eq!(terminal.title.as_ref().unwrap().chars().count(), 240);
    assert!(terminal.bracketed_paste);
    parser.advance(&mut terminal, b"\x1b]2;next\x07\x1b[!p");
    assert_eq!(terminal.title.as_deref(), Some("next"));
    parser.advance(&mut terminal, b"\x1bc");
    assert_eq!(terminal.title, None);
}

#[test]
fn state_parser_crosses_chunks_and_restore_does_not_change_stream_offsets() {
    let mut output = OutputState {
        scrollback: VecDeque::new(),
        base_offset: 0,
        next_offset: 0,
        closed: false,
        state_parser: vte::Parser::new(),
        terminal: TerminalState::default(),
    };

    for chunk in [
        b"prefix\x1b[?9h\x1b[?1002h\x1b[?1003h\x1b[?10".as_slice(),
        b"00h\x1b[?1016h\x1b[?1006h\x1b[?1004;2004h\x1b[?1;66h\x1b[?104".as_slice(),
        b"9h".as_slice(),
    ] {
        let OutputState {
            state_parser,
            terminal,
            ..
        } = &mut output;
        state_parser.advance(terminal, chunk);
        output.append(chunk, 32).unwrap();
    }

    assert!(output.base_offset > 0, "test output must be truncated");
    let snapshot = output.snapshot(None);
    assert!(snapshot.reset);
    assert_eq!(snapshot.offset, output.base_offset);
    assert_eq!(snapshot.next_offset, output.next_offset);
    assert_eq!(
        snapshot.replay.len() as u64,
        output.next_offset - output.base_offset
    );
    let restore = snapshot.restore.expect("truncated reset has mode restore");
    for sequence in [
        "\x1b[?1h",
        "\x1b[?47h",
        "\x1b[?66h",
        "\x1b[?1000h",
        "\x1b[?1004h",
        "\x1b[?1006h",
        "\x1b[?2004h",
    ] {
        assert!(
            restore.contains(sequence),
            "missing {sequence:?} in {restore:?}"
        );
    }
    for replaced in ["\x1b[?9h", "\x1b[?1002h", "\x1b[?1003h", "\x1b[?1016h"] {
        assert!(
            !restore.contains(replaced),
            "replaced mode {replaced:?} leaked into {restore:?}"
        );
    }
    assert!(!restore.contains("1049"), "alt restore must use mode 47");

    let original_next_offset = output.next_offset;
    let original_base_offset = output.base_offset;
    let restore_length = restore.len();
    assert!(restore_length > 0);
    assert_eq!(output.next_offset, original_next_offset);
    assert_eq!(output.base_offset, original_base_offset);
    assert_eq!(
        snapshot.replay.len() as u64,
        snapshot.next_offset - snapshot.offset,
        "restore bytes must not contribute to replay_bytes or offsets"
    );

    let mut complete = OutputState {
        scrollback: VecDeque::new(),
        base_offset: 0,
        next_offset: 0,
        closed: false,
        state_parser: vte::Parser::new(),
        terminal: TerminalState::default(),
    };
    let raw = b"\x1b[?1006hcomplete";
    {
        let OutputState {
            state_parser,
            terminal,
            ..
        } = &mut complete;
        state_parser.advance(terminal, raw);
    }
    complete.append(raw, SCROLLBACK_LIMIT).unwrap();
    let complete_snapshot = complete.snapshot(None);
    assert!(complete_snapshot.reset);
    assert_eq!(complete_snapshot.replay, raw);
    assert_eq!(complete_snapshot.restore, None);

    let mut cleared = TerminalState::default();
    let mut parser = vte::Parser::new();
    parser.advance(
        &mut cleared,
        b"\x1b[?1003h\x1b[?1000l\x1b[?1016h\x1b[?1006l",
    );
    assert_eq!(cleared.mouse_protocol, MouseProtocol::None);
    assert_eq!(cleared.mouse_encoding, MouseEncoding::Default);
    assert!(cleared.restore_sequence().is_empty());
}

#[test]
fn state_parser_tracks_keypad_and_distinguishes_soft_and_hard_reset() {
    let mut modes = TerminalState::default();
    let mut parser = vte::Parser::new();

    parser.advance(&mut modes, b"\x1b=\x1b[?1;47;1003;1016;1004;2004h");
    assert!(modes.application_keypad);
    assert!(modes.application_cursor);
    assert!(modes.alternate_screen);
    assert!(modes.focus_events);
    assert_eq!(modes.mouse_protocol, MouseProtocol::Any);
    assert_eq!(modes.mouse_encoding, MouseEncoding::SgrPixels);
    assert!(modes.bracketed_paste);

    parser.advance(&mut modes, b"\x1b>");
    assert!(
        !modes.application_keypad,
        "DECKPNM disables application keypad"
    );
    parser.advance(&mut modes, b"\x1b=");
    assert!(
        modes.application_keypad,
        "DECKPAM enables application keypad"
    );

    parser.advance(&mut modes, b"\x1b[!p");
    assert!(!modes.application_cursor);
    assert!(!modes.application_keypad);
    assert!(!modes.focus_events);
    assert!(!modes.bracketed_paste);
    assert!(
        modes.alternate_screen,
        "DECSTR must preserve the active screen buffer"
    );
    assert_eq!(
        modes.mouse_protocol,
        MouseProtocol::Any,
        "DECSTR must preserve mouse service state"
    );
    assert_eq!(modes.mouse_encoding, MouseEncoding::SgrPixels);
    assert_eq!(modes.restore_sequence(), "\x1b[?47h\x1b[?1003h\x1b[?1016h");

    parser.advance(&mut modes, b"\x1bc");
    assert!(
        modes.restore_sequence().is_empty(),
        "RIS must reset every tracked mode to default"
    );
    assert!(!modes.alternate_screen);
    assert_eq!(modes.mouse_protocol, MouseProtocol::None);
    assert_eq!(modes.mouse_encoding, MouseEncoding::Default);
}

#[tokio::test]
async fn stalled_owned_send_bounds_force_takeover() {
    let (runtime, _spawned) = test_runtime();
    let first = runtime
        .claim(1, None, false, false)
        .expect("first claim succeeds");
    let ClaimOutcome::Claimed(first) = first else {
        panic!("first attachment did not claim");
    };
    let owner = first.owner;

    let polled = Arc::new(Notify::new());
    let stalled_polled = polled.clone();
    let send_runtime = runtime.clone();
    let stalled = tokio::spawn(async move {
        send_owned(
            &mut PendingSink {
                polled: stalled_polled,
            },
            &send_runtime,
            owner,
            Message::Binary(Bytes::from_static(b"stalled")),
        )
        .await
    });
    polled.notified().await;

    let force_connection = runtime.connection(None, false).unwrap();
    let mut force_sink = CollectingSink {
        messages: Vec::new(),
        first_binary: None,
    };
    let started = tokio::time::Instant::now();
    let forced = tokio::time::timeout(
        SOCKET_SEND_TIMEOUT + Duration::from_secs(1),
        begin_claim(&mut force_sink, &force_connection, true, true, None, false),
    )
    .await
    .expect("force claim must not wait beyond one bounded send");
    assert!(
        started.elapsed() <= SOCKET_SEND_TIMEOUT + Duration::from_millis(500),
        "force takeover exceeded the bounded-send window"
    );
    assert!(!stalled.await.expect("stalled sender task joins"));
    let BeginClaim::Claimed(forced) = forced else {
        panic!("force claim did not replace the owner");
    };
    assert!(runtime.is_owner(forced.owner).unwrap());
}

#[tokio::test]
async fn pending_socket_close_is_bounded() {
    let close_polls = Arc::new(AtomicUsize::new(0));
    let mut sink = PendingCloseSink {
        close_polls: close_polls.clone(),
    };
    let close_timeout = Duration::from_millis(25);
    let started = tokio::time::Instant::now();

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        bounded_close_with_timeout(&mut sink, close_timeout),
    )
    .await
    .expect("bounded close itself must not hang");

    assert!(matches!(result, Err(SocketSendError::Timeout)));
    assert!(close_polls.load(Ordering::Relaxed) > 0);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "pending close exceeded its deadline"
    );
}

#[tokio::test]
async fn replay_catches_up_more_than_broadcast_capacity_without_lag() {
    let (runtime, _spawned) = test_runtime();
    const PREFIX: &[u8] = b"__CATCHUP_PREFIX__";
    runtime.append_output(PREFIX).unwrap();
    let claimed = runtime.claim(1, None, false, false).unwrap();
    let ClaimOutcome::Claimed(claimed) = claimed else {
        panic!("attachment did not claim");
    };
    let stream_offset = claimed.stream_offset;

    let appended = Arc::new(AtomicUsize::new(0));
    let hook_runtime = runtime.clone();
    let hook_appended = appended.clone();
    let hook = Arc::new(move || {
        for index in 0..OUTPUT_CHANNEL_CAPACITY + 37 {
            let bytes = format!("{index:04}|");
            hook_runtime.append_output(bytes.as_bytes()).unwrap();
            hook_appended.fetch_add(bytes.len(), Ordering::Relaxed);
        }
    });
    let mut sink = CollectingSink {
        messages: Vec::new(),
        first_binary: Some(hook),
    };
    let mut active = initialize_stream(&mut sink, &runtime, claimed)
        .await
        .expect("stream initialization catches up");

    let binary = sink
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Binary(bytes) => Some(bytes.as_ref()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let prefix = binary
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)
        .expect("known prefix is present in replay");
    let expected = (0..OUTPUT_CHANNEL_CAPACITY + 37)
        .map(|index| format!("{index:04}|"))
        .collect::<String>();
    let catch_up_start = prefix + PREFIX.len();
    assert_eq!(
        &binary[catch_up_start..catch_up_start + expected.len()],
        expected.as_bytes(),
        "catch-up bytes must remain ordered and contiguous beyond broadcast capacity"
    );
    assert_eq!(appended.load(Ordering::Relaxed), expected.len());
    assert_eq!(
        active.expected_offset,
        stream_offset + u64::try_from(binary.len()).unwrap(),
        "catch-up must preserve the continuous stream offset"
    );
    assert!(
        matches!(
            active.events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ),
        "the retained receiver must start after all catch-up bytes"
    );

    runtime.append_output(b"live-tail").unwrap();
    match active.events.recv().await.unwrap() {
        RuntimeEvent::Output { offset, bytes } => {
            assert_eq!(offset, active.expected_offset);
            assert_eq!(bytes, Bytes::from_static(b"live-tail"));
        }
        other => panic!("unexpected event after catch-up: {other:?}"),
    }
}

#[tokio::test]
async fn live_broadcast_lag_catches_up_without_replacing_the_attachment() {
    let (runtime, _spawned) = test_runtime();
    const PREFIX: &[u8] = b"already-rendered|";
    runtime.append_output(PREFIX).unwrap();
    let claimed = runtime.claim(1, None, false, false).unwrap();
    let ClaimOutcome::Claimed(claimed) = claimed else {
        panic!("attachment did not claim");
    };
    let owner = claimed.owner;
    let mut lagged_events = runtime.events.subscribe();
    let expected = (0..OUTPUT_CHANNEL_CAPACITY + 37)
        .map(|index| format!("{index:04}|"))
        .collect::<String>();
    for chunk in expected.as_bytes().chunks(5) {
        runtime.append_output(chunk).unwrap();
    }
    assert!(matches!(
        lagged_events.recv().await,
        Err(broadcast::error::RecvError::Lagged(_))
    ));

    let mut sink = CollectingSink {
        messages: Vec::new(),
        first_binary: Some({
            let runtime = runtime.clone();
            Arc::new(move || runtime.append_output(b"during-catch-up|").unwrap())
        }),
    };
    let (expected_offset, mut events, output_closed) =
        catch_up_output(&mut sink, &runtime, owner, PREFIX.len() as u64)
            .await
            .expect("retained scrollback repairs a lagged live stream");
    assert!(!output_closed);
    assert_eq!(
        expected_offset,
        (PREFIX.len() + expected.len() + b"during-catch-up|".len()) as u64
    );
    let repaired = sink
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Binary(bytes) => Some(bytes.as_ref()),
            _ => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let mut expected_repair = expected.into_bytes();
    expected_repair.extend_from_slice(b"during-catch-up|");
    assert_eq!(repaired, expected_repair);
    assert!(runtime.is_owner(owner).unwrap());

    runtime.append_output(b"live-tail").unwrap();
    match events.recv().await.unwrap() {
        RuntimeEvent::Output { offset, bytes } => {
            assert_eq!(offset, expected_offset);
            assert_eq!(bytes, Bytes::from_static(b"live-tail"));
        }
        other => panic!("unexpected event after live catch-up: {other:?}"),
    }
}

#[tokio::test]
async fn live_catch_up_stops_promptly_when_runtime_is_deleted() {
    let (runtime, _spawned) = test_runtime();
    let claimed = runtime.claim(1, None, false, false).unwrap();
    let ClaimOutcome::Claimed(claimed) = claimed else {
        panic!("attachment did not claim");
    };
    let owner = claimed.owner;
    runtime
        .append_output(&vec![b'x'; REPLAY_CHUNK_SIZE * 3])
        .unwrap();
    let mut sink = CollectingSink {
        messages: Vec::new(),
        first_binary: Some({
            let runtime = runtime.clone();
            Arc::new(move || runtime.notify_deleted())
        }),
    };

    assert!(
        catch_up_output(&mut sink, &runtime, owner, 0)
            .await
            .is_none()
    );
    let binary_bytes = sink
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::Binary(bytes) => Some(bytes.len()),
            _ => None,
        })
        .sum::<usize>();
    assert_eq!(
        binary_bytes, REPLAY_CHUNK_SIZE,
        "delete should stop catch-up before the next replay chunk"
    );
    assert!(sink.messages.iter().any(|message| match message {
        Message::Text(text) => text.contains("terminal_deleted"),
        _ => false,
    }));
}

#[test]
fn dropping_uncommitted_spawn_kills_and_reaps_shell() {
    let tracker = Arc::new(SpawnTracker::default());
    let spawned = spawn_runtime_blocking(
        "canceled-create".to_owned(),
        TerminalRuntimeSpec {
            cwd: "/".to_owned(),
            shell: "/bin/sh".to_owned(),
            arguments: Vec::new(),
            environment: Default::default(),
            rows: 24,
            cols: 80,
        },
        tracker.begin(),
        None,
    )
    .unwrap();
    let pid = spawned.runtime.child_pid.unwrap() as i32;
    assert_eq!(tracker.count.load(Ordering::Acquire), 1);
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0);

    drop(spawned);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while tracker.count.load(Ordering::Acquire) != 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "uncommitted spawn was not reaped"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ESRCH)
    );
}

#[tokio::test]
async fn graceful_shutdown_waits_for_in_flight_spawn_cleanup() {
    let state = DaemonState::new();
    let permit = {
        let _runtimes = state.lock_runtimes().unwrap();
        state.inner.in_flight_spawns.begin()
    };
    let shutdown_state = state.clone();
    let shutdown = tokio::spawn(async move { shutdown_state.shutdown_all().await });

    tokio::task::yield_now().await;
    assert!(
        !shutdown.is_finished(),
        "shutdown returned while an uncommitted spawn was still in flight"
    );
    assert!(state.inner.shutting_down.load(Ordering::Acquire));

    drop(permit);
    tokio::time::timeout(Duration::from_secs(1), shutdown)
        .await
        .expect("shutdown did not observe spawn cleanup")
        .expect("shutdown task panicked")
        .expect("shutdown failed");
}
