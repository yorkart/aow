//! Owned ACP prompt lifecycle; steering never replaces its request identity.
use super::*;

impl AgentServerStore {
    pub fn start_prompt(&self, id: &str, content: Vec<Content>) -> Result<()> {
        self.start_prompt_inner(id, content, None)
    }

    pub(super) fn start_prompt_inner(
        &self,
        id: &str,
        content: Vec<Content>,
        steering_id: Option<&str>,
    ) -> Result<()> {
        ensure!(!content.is_empty(), "Prompt is empty");
        let connection = self.session_connection(id)?;
        ensure!(connection.connected(), "ACP adapter is disconnected");
        let content = serde_json::to_value(content)?;
        // SDK validation stays internal to the stable facade.
        let blocks: Vec<agent_client_protocol::schema::v2::ContentBlock> =
            serde_json::from_value(content.clone())?;
        crate::acp_thread::content::validate_prompt_content_for_v1(&blocks)?;
        ensure!(
            !blocks.iter().any(|block| matches!(
                block,
                agent_client_protocol::schema::v2::ContentBlock::Image(_)
            )) || connection.info.prompt_capabilities.get("image").is_some(),
            "This agent does not support images"
        );
        let mut threads = self.inner.threads.lock().unwrap();
        ensure!(!threads.forking.contains(id), "ACP session is being forked");
        let thread = threads.threads.get(id).context("ACP session not found")?;
        let thread = thread.lock().unwrap();
        let admitted = match steering_id {
            None => thread.status == "idle" && !thread.steering_pending,
            Some(id) => thread.steering.pending.as_ref().is_some_and(|pending| {
                pending.id == id && pending.prompt_required && !pending.cancelled
            }),
        };
        ensure!(
            admitted
                && thread.status != "disconnected"
                && thread.active_prompt.is_none()
                && !thread.steering.detached
                && !thread.needs_load
                && !thread
                    .recovery
                    .as_ref()
                    .is_some_and(|recovery| recovery.phase != "failed"),
            "ACP session is busy or disconnected"
        );
        let session_id = thread
            .session_id
            .clone()
            .context("ACP session has not been created")?;
        let prompt_request = crate::acp_thread::content::prompt_to_v1(
            agent_client_protocol::schema::v2::PromptRequest::new(
                agent_client_protocol::schema::v2::SessionId::new(session_id.clone()),
                blocks,
            ),
        )?;
        let previous = thread.clone();
        let mut entry_start = if steering_id.is_some() {
            thread.steering.pending.as_ref().unwrap().entry_start
        } else {
            thread.entries.len()
        };
        let previous_usage = thread.last_prompt_usage.clone().unwrap_or(Value::Null);
        drop(thread);
        let turn_id = uuid::Uuid::new_v4().to_string();
        if let Err(error) = threads.update(id, |thread| {
            thread.interacted_at = Some(chrono::Utc::now().to_rfc3339());
            thread.active_prompt = Some(turn_id.clone());
            thread.steering.activity = None;
            thread.status = "working".into();
            thread.error = None;
            thread.stop_reason = None;
            thread.pending_user_echo = content
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|block| block["text"].as_str())
                .collect::<String>();
            thread.pending_user_images = content
                .as_array()
                .into_iter()
                .flatten()
                .filter(|block| block["type"] == "image")
                .cloned()
                .collect();
            if let Some(steering_id) = steering_id {
                entry_start = crate::acp_thread::steering::relocate_input(
                    thread,
                    entry_start,
                    content.as_array().unwrap().len(),
                    thread.entries.len(),
                );
                crate::acp_thread::steering::forget_echo(thread, steering_id);
                crate::acp_thread::steering::delivery(thread, entry_start, "started", None);
                for entry in thread
                    .entries
                    .iter_mut()
                    .skip(entry_start)
                    .take(content.as_array().unwrap().len())
                {
                    entry.kind = "user".into();
                }
                thread.steering.pending = None;
                thread.steering_pending = false;
            } else {
                for block in content.as_array().into_iter().flatten() {
                    AcpThread::push(thread, "user", block.clone());
                }
            }
            crate::acp_thread::turns::start(thread, entry_start);
            if thread.title == "New conversation" {
                thread.title = content[0]["text"]
                    .as_str()
                    .unwrap_or("Conversation")
                    .chars()
                    .take(80)
                    .collect();
            }
        }) {
            // No prompt was sent. Replace the failed pending save so retrying cannot leave
            // an unsent user message or a permanently working session in memory/on disk.
            let message = format!("{error:#}");
            let _ = threads.update(id, |thread| {
                let revision = thread.revision;
                *thread = previous;
                thread.revision = revision;
                thread.error = Some(message);
            });
            return Err(error);
        }
        // Keep admission locked until enqueueing, so stop cannot overtake an
        // idle steering fallback (or an ordinary prompt).
        let pending = connection.begin_prompt(prompt_request);
        drop(threads);
        let service = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let result = pending.block_task().await.and_then(|result| {
                serde_json::to_value(result)
                    .map_err(agent_client_protocol::Error::into_internal_error)
            });
            let completed = {
                let mut threads = service.inner.threads.lock().unwrap();
                if !threads.threads.get(&id).is_some_and(|thread| {
                    thread.lock().unwrap().active_prompt.as_ref() == Some(&turn_id)
                }) {
                    return;
                }
                let usage = result
                    .as_ref()
                    .ok()
                    .and_then(|result| result.get("usage"))
                    .cloned();
                let succeeded = result.is_ok();
                let mut completed = None;
                let saved = threads.update(&id, |thread| {
                    AcpThread::clear_session_meta(thread);
                    thread.active_prompt = None;
                    thread.last_prompt_usage = usage.clone();
                    thread.pending_user_echo.clear();
                    thread.pending_user_images.clear();
                    thread.status = if connection.connected() && thread.status != "disconnected" {
                        "idle"
                    } else {
                        "disconnected"
                    }
                    .into();
                    let later_turn = thread.steering.detached
                        || thread.turns.last().is_some_and(|turn| {
                            turn.user_entry_id != thread.entries[entry_start].id
                        });
                    let later_stop = thread.stop_reason.clone();
                    let later_error = thread.error.clone();
                    match result {
                        Ok(result) => {
                            thread.stop_reason = result["stopReason"].as_str().map(str::to_owned);
                            if let Some(usage) = result.get("usage") {
                                thread.usage = usage.clone();
                            }
                        }
                        Err(error) => {
                            thread.auth_required =
                                error.code == agent_client_protocol::ErrorCode::AuthRequired;
                            thread.error = Some(format!("{error:#}"));
                        }
                    }
                    crate::acp_thread::turns::finish_at(thread, entry_start);
                    if later_turn {
                        thread.stop_reason = later_stop;
                        thread.error = later_error;
                    }
                    crate::acp_thread::steering::settle_status(thread);
                    if succeeded {
                        completed = completion::completed_turn(
                            thread,
                            turn_id.clone(),
                            entry_start,
                            usage.clone(),
                            &previous_usage,
                        );
                        if thread.steering_pending {
                            thread.steering.completion = completed.take();
                        }
                    }
                });
                if let Err(error) = saved {
                    tracing::error!(%error, "Failed to save ACP prompt result");
                    None
                } else {
                    completed
                }
            };
            service.start_steering_fallback(&id);
            if let Some(completed) = completed {
                service.inner.host.turn_completed(completed).await;
            }
        });
        Ok(())
    }
}
