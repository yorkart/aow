use super::*;

pub(super) async fn bridge_terminal_socket(
    browser: WebSocket,
    daemon: TerminaldAttachStream,
    manager: TerminalManager,
    pane_id: String,
    access: Option<crate::auth::SessionAccess>,
    events: crate::workspace_events::WorkspaceEvents,
) {
    let Ok(hosting_gate) = manager.hosting_gate(&pane_id) else {
        return;
    };
    let (mut browser_tx, mut browser_rx) = browser.split();
    let (mut daemon_tx, mut daemon_rx) = daemon.split();
    let mut resize_syncs = FuturesUnordered::new();
    let mut resize_sync_pending = false;
    let cli_pane = manager.cli_agent(&pane_id).is_ok();
    let mut browser_control_guard = None;
    let revoked = async {
        match &access {
            Some(access) => access.revoked().await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::pin!(revoked);
    loop {
        tokio::select! {
            biased;
            _ = &mut revoked => {
                let _ = send_bridge_message(&mut browser_tx, Message::Close(None), BRIDGE_CLOSE_FORWARD_TIMEOUT).await;
                break;
            },
            browser_message = browser_rx.next() => match browser_message {
                Some(Ok(message)) => {
                    // Polling closes idle connections, while checking each input
                    // prevents commands racing with a password change or logout.
                    if access.as_ref().is_some_and(|access| access.validate(matches!(message, Message::Binary(_) | Message::Text(_))).is_err()) {
                        let _ = send_bridge_message(&mut browser_tx, Message::Close(None), BRIDGE_CLOSE_FORWARD_TIMEOUT).await;
                        break;
                    }
                    let close = matches!(message, Message::Close(_));
                    let mut message = axum_to_tungstenite(message);
                    // Serialize takeover with the entire paste + Enter submission.
                    // Revalidate on every frame so previously connected controllers
                    // cannot write after another client enables hosting.
                    let _hosting_gate = hosting_gate.lock().await;
                    if !close {
                        use aow_protocol::TerminalAttachClientMessage as ClientMessage;
                        let command = if let tungstenite::Message::Text(text) = &message {
                            serde_json::from_str::<ClientMessage>(text).ok()
                        } else { None };
                        let hosted = match manager.hosting(&pane_id) {
                            Ok(hosting) => hosting.is_some(),
                            Err(_) => break,
                        };
                        if hosted {
                            match command {
                                Some(ClientMessage::Claim { force: true }) => {
                                    if manager.set_hosting(&pane_id, None, None).is_err() { break; }
                                    events.terminals_changed();
                                }
                                Some(ClientMessage::Claim { force: false }) => {
                                    message = tungstenite::Message::Text(serde_json::to_string(&ClientMessage::Observe).unwrap().into());
                                }
                                Some(ClientMessage::Observe) => {}
                                _ if matches!(message, tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)) => {}
                                _ => continue,
                            }
                        }
                    }
                    if cli_pane && !close {
                        use aow_protocol::TerminalAttachClientMessage as ClientMessage;
                        let command = if let tungstenite::Message::Text(text) = &message {
                            serde_json::from_str::<ClientMessage>(text).ok()
                        } else { None };
                        match command {
                            Some(ClientMessage::Claim { force: false }) if browser_control_guard.is_none() => {
                                message = tungstenite::Message::Text(serde_json::to_string(&ClientMessage::Observe).unwrap().into());
                            }
                            Some(ClientMessage::Claim { force: true }) => {
                                let lease = manager.agent_operation(&pane_id).and_then(|operation| operation.try_read_owned().map_err(|_| TerminalError::Conflict("Agent 正在初始化或提交任务，只能旁观".into())));
                                if manager.cli_agent(&pane_id).is_ok_and(|info| info.state.phase == aow_protocol::AgentTerminalPhase::Ready) && lease.is_ok() {
                                    browser_control_guard = lease.ok();
                                } else {
                                    // Reconnect as an observer; never let force bypass initialization.
                                    message = tungstenite::Message::Text(serde_json::to_string(&ClientMessage::Observe).unwrap().into());
                                }
                            }
                            Some(ClientMessage::Observe) => { browser_control_guard = None; }
                            Some(ClientMessage::Resize { .. } | ClientMessage::Write { .. }) | None
                                if browser_control_guard.is_none() && !matches!(message, tungstenite::Message::Ping(_) | tungstenite::Message::Pong(_)) => {
                                    continue;
                                }
                            _ => {}
                        }
                    }
                    if close {
                        match send_bridge_message(
                            &mut daemon_tx,
                            message,
                            BRIDGE_CLOSE_FORWARD_TIMEOUT,
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::debug!(%error, %pane_id, "failed to forward browser close to terminald");
                            }
                            Err(_) => {
                                tracing::debug!(%pane_id, "timed out forwarding browser close to terminald");
                            }
                        }
                        break;
                    }
                    match send_bridge_message(
                        &mut daemon_tx,
                        message,
                        BRIDGE_MESSAGE_FORWARD_TIMEOUT,
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::debug!(%error, %pane_id, "failed to forward browser message to terminald");
                            break;
                        }
                        Err(_) => {
                            tracing::debug!(%pane_id, "timed out forwarding browser message to terminald");
                            break;
                        }
                    }
                }
                Some(Err(_)) | None => break,
            },
            daemon_message = daemon_rx.next() => match daemon_message {
                Some(Ok(tungstenite::Message::Frame(_))) => {}
                Some(Ok(message)) => {
                    let mut sync_after_forward = false;
                    if let tungstenite::Message::Text(text) = &message
                        && let Ok(control) = serde_json::from_str::<TerminalAttachServerMessage>(text)
                    {
                        let result = match control {
                            TerminalAttachServerMessage::Status { status, exit_code } => {
                                manager.apply_attach_status(&pane_id, status, exit_code)
                            }
                            TerminalAttachServerMessage::Resized { .. } => {
                                // ACKs from superseded and current bridges can arrive in either
                                // order. Read terminald's authoritative size so a late old ACK
                                // can never overwrite the newest controller's dimensions. Defer
                                // starting the query until after the ACK is forwarded.
                                sync_after_forward = true;
                                Ok(())
                            }
                            TerminalAttachServerMessage::Written { .. }
                            | TerminalAttachServerMessage::Control { .. }
                            | TerminalAttachServerMessage::Stream { .. }
                            | TerminalAttachServerMessage::Error { .. } => Ok(()),
                        };
                        if let Err(error) = result {
                            tracing::warn!(%error, %pane_id, "failed to persist terminal control state");
                        }
                    }
                    let close = matches!(message, tungstenite::Message::Close(_));
                    let message = tungstenite_to_axum(message);
                    if close {
                        match send_bridge_message(
                            &mut browser_tx,
                            message,
                            BRIDGE_CLOSE_FORWARD_TIMEOUT,
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => {
                                tracing::debug!(%error, %pane_id, "failed to forward terminald close to browser");
                            }
                            Err(_) => {
                                tracing::debug!(%pane_id, "timed out forwarding terminald close to browser");
                            }
                        }
                        break;
                    }
                    match send_bridge_message(
                        &mut browser_tx,
                        message,
                        BRIDGE_MESSAGE_FORWARD_TIMEOUT,
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => {
                            tracing::debug!(%error, %pane_id, "failed to forward terminald message to browser");
                            break;
                        }
                        Err(_) => {
                            tracing::debug!(%pane_id, "timed out forwarding terminald message to browser");
                            break;
                        }
                    }
                    if sync_after_forward {
                        // Keep at most one query in flight and one coalesced follow-up so resize
                        // persistence never stalls socket forwarding or grows without bound.
                        if resize_syncs.is_empty() {
                            resize_syncs.push(sync_bridge_runtime(
                                manager.clone(),
                                pane_id.clone(),
                                "after resize",
                            ));
                        } else {
                            resize_sync_pending = true;
                        }
                    }
                }
                Some(Err(error)) => {
                    tracing::debug!(%error, %pane_id, "terminald attach stream stopped");
                    break;
                }
                None => break,
            },
            Some(()) = resize_syncs.next() => {
                if resize_sync_pending {
                    resize_sync_pending = false;
                    resize_syncs.push(sync_bridge_runtime(
                        manager.clone(),
                        pane_id.clone(),
                        "after coalesced resize",
                    ));
                }
            },
        }
    }
    // Release the daemon attachment before doing any follow-up API request. In
    // particular, a disconnected browser must relinquish terminal control even
    // if runtime synchronization is delayed or terminald is unavailable.
    drop(daemon_tx);
    drop(daemon_rx);
    // Cancel any in-flight resize query before the final authoritative query.
    // The future is owned by this bridge, so it cannot outlive the attachment.
    drop(resize_syncs);
    sync_bridge_runtime(manager, pane_id, "after attach").await;
}

pub(super) async fn send_bridge_message<S, Item>(
    sink: &mut S,
    message: Item,
    timeout: Duration,
) -> Result<Result<(), S::Error>, tokio::time::error::Elapsed>
where
    S: futures_util::Sink<Item> + Unpin,
{
    tokio::time::timeout(timeout, sink.send(message)).await
}

async fn sync_bridge_runtime(manager: TerminalManager, pane_id: String, context: &'static str) {
    match bounded_sync_runtime(&manager, &pane_id, BRIDGE_RUNTIME_SYNC_TIMEOUT).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, %pane_id, context, "failed to persist terminal runtime status");
        }
        Err(_) => {
            tracing::warn!(%pane_id, context, "timed out persisting terminal runtime status");
        }
    }
}

pub(super) async fn bounded_sync_runtime(
    manager: &TerminalManager,
    pane_id: &str,
    timeout: Duration,
) -> Result<Result<(), TerminalError>, tokio::time::error::Elapsed> {
    tokio::time::timeout(timeout, manager.sync_runtime(pane_id)).await
}

fn axum_to_tungstenite(message: Message) -> tungstenite::Message {
    match message {
        Message::Text(text) => tungstenite::Message::Text(text.to_string().into()),
        Message::Binary(bytes) => tungstenite::Message::Binary(bytes),
        Message::Ping(bytes) => tungstenite::Message::Ping(bytes),
        Message::Pong(bytes) => tungstenite::Message::Pong(bytes),
        Message::Close(frame) => {
            tungstenite::Message::Close(frame.map(|frame| tungstenite::protocol::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }))
        }
    }
}

fn tungstenite_to_axum(message: tungstenite::Message) -> Message {
    match message {
        tungstenite::Message::Text(text) => Message::Text(text.to_string().into()),
        tungstenite::Message::Binary(bytes) => Message::Binary(bytes),
        tungstenite::Message::Ping(bytes) => Message::Ping(bytes),
        tungstenite::Message::Pong(bytes) => Message::Pong(bytes),
        tungstenite::Message::Close(frame) => {
            Message::Close(frame.map(|frame| axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }))
        }
        tungstenite::Message::Frame(_) => unreachable!("raw frames are filtered by the bridge"),
    }
}
