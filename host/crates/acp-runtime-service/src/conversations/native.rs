use super::{Conversations, send};
use agent_client_protocol::schema::v2 as acp;
use anyhow::{Result, bail, ensure};
use aow_acp_runtime_core::{AcpService, SyncResource};
use axum::extract::{State, WebSocketUpgrade, ws::Message};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Duration};
use tokio::sync::mpsc;

pub(super) async fn upgrade(
    State(state): State<Conversations>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    let mut changes = state.service.subscribe_sync();
    let mut protocol = state.service.subscribe_protocol();
    aow_i18n::http::upgrade(
        ws.max_message_size(32 * 1024 * 1024),
        move |mut socket| async move {
            let (output, mut responses) = mpsc::channel::<Value>(128);
            let mut stopping = state.stopping.clone();
            let mut initialized = false;
            let mut permissions = HashMap::<String, (String, bool)>::new();
            let mut workers = tokio::task::JoinSet::new();
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            heartbeat.tick().await;
            let mut last_seen = tokio::time::Instant::now();
            loop {
                tokio::select! {
                    biased;
                    _ = stopping.changed() => break,
                    notification = protocol.recv(), if initialized => {
                        let Ok(notification) = notification else { break; };
                        if send(&mut socket, Message::Text(notification.to_string().into())).await.is_err() { break; }
                    },
                    response = responses.recv() => {
                        let Some(response) = response else { break; };
                        if send(&mut socket, Message::Text(response.to_string().into())).await.is_err() { break; }
                    },
                    _ = workers.join_next(), if !workers.is_empty() => {},
                    change = changes.recv(), if initialized => {
                        let Ok(change) = change else { break; };
                        let requests = match change.resource {
                            SyncResource::Session { thread_id } => state.service.snapshot(&thread_id).ok().map(|snapshot| (thread_id, false, snapshot.data.permissions)),
                            SyncResource::Requests { connection_id } => state.service.connection_requests(&connection_id).ok().map(|requests| (connection_id, true, requests)),
                            _ => None,
                        };
                        if let Some((owner, connection, requests)) = requests {
                            for request in requests {
                                if permissions.contains_key(&request.id) { continue; }
                                permissions.insert(request.id.clone(), (owner.clone(), connection));
                                let mut params = request.request;
                                if !connection { params["sessionId"] = json!(owner); }
                                else if let Some(session) = params["sessionId"].as_str()
                                    && let Ok(thread) = state.service.thread_for_session(&owner, session) {
                                    params["sessionId"] = json!(thread);
                                }
                                let message = json!({"jsonrpc":"2.0","id":request.id,"method":if request.kind == "elicitation" {"elicitation/create"} else {"session/request_permission"},"params":params});
                                if send(&mut socket, Message::Text(message.to_string().into())).await.is_err() { return; }
                            }
                        }
                    },
                    message = socket.recv() => {
                        last_seen = tokio::time::Instant::now();
                        let text = match message {
                            Some(Ok(Message::Text(text))) => text,
                            Some(Ok(Message::Ping(data))) => { if send(&mut socket, Message::Pong(data)).await.is_err() { break; } continue; },
                            Some(Ok(Message::Pong(_))) => continue,
                            _ => break,
                        };
                        let Ok(message) = serde_json::from_str::<Value>(&text) else { break; };
                        if message["jsonrpc"] != "2.0" { break; }
                        let id = message["id"].clone();
                        if !initialized {
                            if message["method"] != "initialize" || id.is_null() { break; }
                            if serde_json::from_value::<agent_client_protocol::schema::v1::InitializeRequest>(message["params"].clone()).is_err() { break; }
                            for session in state.service.sessions(None) {
                                if let Ok(snapshot) = state.service.snapshot(&session.thread_id) {
                                    for request in &snapshot.permissions { permissions.insert(request.id.clone(), (session.thread_id.clone(), false)); }
                                }
                                if let Ok(Some(connection)) = state.service.session_connection_info(&session.thread_id)
                                    && let Ok(requests) = state.service.connection_requests(&connection.id) {
                                        for request in requests { permissions.entry(request.id).or_insert((connection.id.clone(), true)); }
                                }
                            }
                            initialized = true;
                            let result = json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true},"authMethods":[],"_meta":{"aow":{"admission":true}}}});
                            if send(&mut socket, Message::Text(result.to_string().into())).await.is_err() { break; }
                            continue;
                        }
                        if message.get("method").is_none() {
                            let key = id.as_str().unwrap_or_default();
                            let result = if let Some((owner, connection)) = permissions.remove(key) {
                                if connection { state.service.answer_connection(&owner,key,message["result"].clone()) }
                                else { state.service.answer(&owner,key,message["result"].clone()) }
                            } else { Err(anyhow::anyhow!("Unknown permission request")) };
                            let response = match result { Ok(()) => json!({"jsonrpc":"2.0","method":"_aow/answered","params":{"id":id}}), Err(e) => failure(id,e) };
                            if send(&mut socket, Message::Text(response.to_string().into())).await.is_err() { break; }
                            continue;
                        }
                        if workers.len() >= 128 {
                            let response = failure(id, anyhow::anyhow!("Too many pending ACP operations"));
                            if send(&mut socket, Message::Text(response.to_string().into())).await.is_err() { break; }
                            continue;
                        }
                        let service = state.service.clone();
                        let output = output.clone();
                        let request_language = aow_i18n::current_language();
                        workers.spawn(async move {
                            let language = message["params"]["_meta"]["aow"]["language"].as_str().map(aow_i18n::Language::parse).unwrap_or(request_language);
                            let result = aow_i18n::scope(language, dispatch(&service, &message, &output)).await;
                            // session/cancel is a notification. It never receives a response.
                            if !id.is_null() {
                                let response = match result { Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}), Err(error) => failure(id,error) };
                                let _ = output.send(response).await;
                            }
                        });
                    },
                    _ = heartbeat.tick() => {
                        if last_seen.elapsed() > Duration::from_secs(45) || send(&mut socket, Message::Ping(vec![].into())).await.is_err() { break; }
                    }
                }
            }
            // Dropping the gateway never cancels an admitted daemon-owned turn.
        },
    )
}
fn failure(id: Value, error: anyhow::Error) -> Value {
    let error = error
        .downcast::<agent_client_protocol::Error>()
        .unwrap_or_else(|error| {
            agent_client_protocol::Error::internal_error().data(format!("{error:#}"))
        });
    json!({"jsonrpc":"2.0","id":id,"error":error})
}
async fn dispatch(
    service: &AcpService,
    message: &Value,
    output: &mpsc::Sender<Value>,
) -> Result<Value> {
    let params = message["params"].clone();
    match message["method"].as_str().unwrap_or_default() {
        "session/new" => {
            let request: agent_client_protocol::schema::v1::NewSessionRequest =
                serde_json::from_value(params.clone())?;
            ensure!(
                request.mcp_servers.is_empty(),
                "Per-session MCP servers are configured on the runtime"
            );
            let agent = service
                .select_agent_id(params["_meta"]["aow"]["agentId"].as_str())
                .await?;
            let prepared = service.prepare_session(&agent, request.cwd).await?;
            let snapshot = service.resume(&prepared.thread_id).await?;
            Ok(
                json!({"sessionId":snapshot.thread_id,"modes":snapshot.modes,"configOptions":snapshot.config_options}),
            )
        }
        "session/load" => {
            let request: agent_client_protocol::schema::v1::LoadSessionRequest =
                serde_json::from_value(params)?;
            let thread = request.session_id.to_string();
            ensure!(
                service.session_info(&thread)?.cwd == request.cwd.to_string_lossy(),
                "Session belongs to another workspace"
            );
            let snapshot = service.resume(&thread).await?;
            Ok(json!({"modes":snapshot.modes,"configOptions":snapshot.config_options}))
        }
        "session/prompt" => {
            let request: acp::PromptRequest = serde_json::from_value(params.clone())?;
            let thread = request.session_id.to_string();
            let content = serde_json::from_value(serde_json::to_value(request.prompt)?)?;
            let request_id = params["_meta"]["aow"]["requestId"].as_str();
            match request_id {
                Some(id) => service.start_prompt_once(&thread, id, content)?,
                None => service.start_prompt(&thread, content)?,
            }
            let turn_id = service
                .snapshot(&thread)?
                .turns
                .last()
                .map(|turn| turn.user_entry_id.clone());
            output
                .send(
                    json!({"jsonrpc":"2.0","method":"_aow/admitted","params":{"id":message["id"]}}),
                )
                .await?;
            loop {
                let snapshot = service.snapshot(&thread)?;
                let status = match request_id {
                    Some(id) => snapshot
                        .prompt_receipts
                        .get(id)
                        .map(|receipt| (&receipt.status, &receipt.error, &receipt.stop_reason)),
                    None => snapshot
                        .turns
                        .iter()
                        .find(|turn| Some(&turn.user_entry_id) == turn_id.as_ref())
                        .map(|turn| (&turn.status, &turn.error, &turn.stop_reason)),
                };
                let Some((status, error, stop_reason)) = status else {
                    bail!("Prompt outcome disappeared");
                };
                if status != "in_progress" {
                    if let Some(error) = error {
                        bail!("{error}");
                    }
                    return Ok(json!({"stopReason":stop_reason.as_deref().unwrap_or("end_turn")}));
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        "session/cancel" => {
            let request: agent_client_protocol::schema::v1::CancelNotification =
                serde_json::from_value(params)?;
            service.cancel(&request.session_id.to_string())?;
            Ok(Value::Null)
        }
        "session/set_mode" => {
            let request: agent_client_protocol::schema::v1::SetSessionModeRequest =
                serde_json::from_value(params)?;
            service
                .set_mode(
                    &request.session_id.to_string(),
                    &request.mode_id.to_string(),
                )
                .await?;
            Ok(json!({}))
        }
        "session/set_config_option" => {
            let session = params["sessionId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing sessionId"))?;
            let option = params["configId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing configId"))?;
            service
                .set_config_option(session, option, params["value"].clone())
                .await?;
            Ok(json!({"configOptions":service.snapshot(session)?.config_options}))
        }
        "authenticate" => {
            let request: agent_client_protocol::schema::v1::AuthenticateRequest =
                serde_json::from_value(params.clone())?;
            let connection = params["_meta"]["aow"]["connectionId"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing connectionId"))?;
            service
                .authenticate(connection, &request.method_id.to_string())
                .await?;
            Ok(json!({}))
        }
        "_aow/steer" => {
            let request: acp::PromptRequest = serde_json::from_value(params)?;
            service.start_steering(
                &request.session_id.to_string(),
                serde_json::from_value(serde_json::to_value(request.prompt)?)?,
            )?;
            Ok(json!({}))
        }
        _ => Err(agent_client_protocol::Error::method_not_found().into()),
    }
}
