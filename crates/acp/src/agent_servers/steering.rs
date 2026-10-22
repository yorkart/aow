//! Capability-negotiated mid-turn input; ordinary prompt responses retain ownership.
use super::*;
use crate::acp_thread::steering::{self, PendingSteer};
use crate::facade::InputDelivery;

impl AgentServerStore {
    pub fn start_steering(&self, id: &str, content: Vec<Content>) -> Result<()> {
        ensure!(!content.is_empty(), "Prompt is empty");
        let connection = self.session_connection(id)?;
        ensure!(connection.connected(), "ACP adapter is disconnected");
        ensure!(
            connection.info.extensions["steering"]["supported"] == true,
            "This agent does not support steering"
        );
        let original = serde_json::to_value(&content)?;
        let blocks: Vec<agent_client_protocol::schema::v2::ContentBlock> =
            serde_json::from_value(original.clone())?;
        crate::acp_thread::content::validate_prompt_content_for_v1(&blocks)?;
        ensure!(
            !blocks.iter().any(|block| matches!(
                block,
                agent_client_protocol::schema::v2::ContentBlock::Image(_)
            )) || connection.info.prompt_capabilities.get("image").is_some(),
            "This agent does not support images"
        );
        let mut threads = self.inner.threads.lock().unwrap();
        threads.ensure_session_available(id)?;
        let source = threads.threads.get(id).context("ACP session not found")?;
        let thread = source.lock().unwrap();
        ensure!(
            !thread.steering_pending,
            "A steering request is already pending"
        );
        if thread.status == "idle" {
            drop(thread);
            drop(threads);
            return self.start_prompt(id, content);
        }
        ensure!(
            thread.status == "working"
                && !thread.needs_load
                && thread
                    .recovery
                    .as_ref()
                    .is_none_or(|recovery| recovery.phase == "failed"),
            "ACP session is busy or disconnected"
        );
        let session_id = thread
            .session_id
            .clone()
            .context("ACP session has not been created")?;
        let wire = crate::acp_thread::content::prompt_to_v1(
            agent_client_protocol::schema::v2::PromptRequest::new(
                agent_client_protocol::schema::v2::SessionId::new(session_id.clone()),
                blocks,
            ),
        )?;
        let mut params = serde_json::to_value(wire)?;
        // Agents that implement this opt-in leave idle input unconsumed. Older
        // implementations can still return startedNewTurn; never resend it.
        params["_meta"] = json!({"steering":{"idleBehavior":"promptRequired"}});
        let request_id = uuid::Uuid::new_v4().to_string();
        let entry_start = thread.entries.len();
        let previous = thread.clone();
        drop(thread);
        let saved = threads
            .update(id, |thread| {
                thread.steering_pending = true;
                thread.interacted_at = Some(chrono::Utc::now().to_rfc3339());
                thread.steering.pending = Some(PendingSteer {
                    id: request_id.clone(),
                    entry_start,
                    content,
                    prompt_required: false,
                    cancelled: false,
                    saw_idle: thread.steering.activity == Some(steering::Activity::Idle),
                    saw_active: false,
                    terminal: None,
                    new_turn_start: None,
                });
                steering::expect_echo(
                    thread,
                    request_id.clone(),
                    params["prompt"].as_array().unwrap(),
                );
                for block in original.as_array().unwrap() {
                    AcpThread::push(thread, "steering", block.clone());
                    thread.entries.last_mut().unwrap().delivery = Some(InputDelivery {
                        status: "sending".into(),
                        error: None,
                    });
                }
            })
            .and_then(|_| threads.metadata.flush());
        if let Err(error) = saved {
            let _ = threads.update(id, |thread| {
                let revision = thread.revision;
                *thread = previous;
                thread.revision = revision;
            });
            return Err(error);
        }
        // Send before releasing admission, so an immediately following stop
        // cannot overtake this input. The task survives an HTTP client leaving.
        let pending = connection.begin_steering(id, params)?;
        drop(threads);
        let service = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let result = pending.block_task().await;
            let mut cancel_fallback = false;
            let mut completed = None;
            let saved = {
                let mut threads = service.inner.threads.lock().unwrap();
                if !threads.threads.get(&id).is_some_and(|thread| {
                    thread
                        .lock()
                        .unwrap()
                        .steering
                        .pending
                        .as_ref()
                        .is_some_and(|pending| pending.id == request_id)
                }) {
                    return;
                }
                threads.update(&id, |thread| {
                    let mut pending = thread.steering.pending.take().unwrap();
                    thread.steering_pending = false;
                    match result {
                        Ok(ref response) if response["outcome"] == "injected" => {
                            steering::delivery(thread, entry_start, "injected", None);
                        }
                        Ok(ref response) if response["outcome"] == "startedNewTurn" => {
                            let entry_start = steering::relocate_input(
                                thread,
                                entry_start,
                                pending.content.len(),
                                pending.new_turn_start.unwrap_or(entry_start),
                            );
                            steering::delivery(thread, entry_start, "started", None);
                            for entry in thread
                                .entries
                                .iter_mut()
                                .skip(entry_start)
                                .take(pending.content.len())
                            {
                                entry.kind = "user".into();
                            }
                            crate::acp_thread::turns::start(thread, entry_start);
                            thread.stop_reason = None;
                            thread.error = None;
                            thread.steering.detached = true;
                            thread.steering.detached_active = pending.saw_active;
                            thread.steering.cancelled = pending.cancelled;
                            thread.steering_untracked = !pending.saw_active;
                            cancel_fallback = pending.cancelled;
                            if let Some(terminal) = pending.terminal {
                                steering::finish_detached(thread, terminal);
                            }
                        }
                        Ok(ref response)
                            if response["outcome"] == "promptRequired"
                                && response["reason"] == "noRunningTurn" =>
                        {
                            steering::forget_echo(thread, &request_id);
                            if pending.cancelled {
                                steering::delivery(thread, entry_start, "cancelled", None);
                            } else {
                                if thread.steering.detached {
                                    steering::finish_detached(thread, steering::Activity::Idle);
                                }
                                pending.prompt_required = true;
                                thread.steering_pending = true;
                                thread.steering.pending = Some(pending);
                            }
                        }
                        Ok(ref response) if response["outcome"] == "failed" => {
                            steering::forget_echo(thread, &request_id);
                            steering::delivery(
                                thread,
                                entry_start,
                                "failed",
                                Some("Agent 未接受追加指令。".into()),
                            );
                        }
                        Err(ref error)
                            if matches!(
                                error.code,
                                agent_client_protocol::ErrorCode::MethodNotFound
                                    | agent_client_protocol::ErrorCode::InvalidParams
                                    | agent_client_protocol::ErrorCode::InvalidRequest
                            ) =>
                        {
                            steering::forget_echo(thread, &request_id);
                            steering::delivery(
                                thread,
                                entry_start,
                                "failed",
                                Some(format!("{error:#}")),
                            );
                        }
                        ref other => {
                            steering::delivery(
                                thread,
                                entry_start,
                                "unknown",
                                Some(format!("无法确认追加指令是否送达，请勿自动重发：{other:?}")),
                            );
                        }
                    }
                    steering::settle_status(thread);
                    completed = thread.steering.completion.take();
                })
            };
            if let Err(error) = saved {
                tracing::error!(%error, "Failed to save ACP steering result");
            }
            if cancel_fallback {
                let _ = connection.cancel(&session_id);
            }
            service.start_steering_fallback(&id);
            if let Some(completed) = completed {
                service.inner.host.turn_completed(completed).await;
            }
        });
        Ok(())
    }

    pub(super) fn start_steering_fallback(&self, id: &str) {
        let fallback = self.snapshot(id).ok().and_then(|thread| {
            thread.steering.pending.filter(|pending| {
                pending.prompt_required && !pending.cancelled && thread.active_prompt.is_none()
            })
        });
        if let Some(pending) = fallback
            && let Err(error) = self.start_prompt_inner(id, pending.content, Some(&pending.id))
        {
            let _ = self.inner.threads.lock().unwrap().update(id, |thread| {
                if thread
                    .steering
                    .pending
                    .as_ref()
                    .is_some_and(|active| active.id == pending.id)
                {
                    steering::delivery(
                        thread,
                        pending.entry_start,
                        "failed",
                        Some(error.to_string()),
                    );
                    thread.steering.pending = None;
                    thread.steering_pending = false;
                    steering::settle_status(thread);
                }
            });
        }
    }
}
