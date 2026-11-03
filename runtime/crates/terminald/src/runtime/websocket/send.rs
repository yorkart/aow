use super::*;

pub(in crate::runtime) async fn send_owned<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: ControllerOwner,
    message: Message,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    let _gate = runtime.delivery_gate.lock().await;
    if !runtime.is_owner(owner).unwrap_or(false) {
        let _ = send_attachment_superseded(sender).await;
        return false;
    }
    bounded_send(sender, message).await.is_ok()
}

pub(super) async fn send_stream<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    message: Message,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    match owner {
        Some(owner) => send_owned(sender, runtime, owner, message).await,
        None => bounded_send(sender, message).await.is_ok(),
    }
}

pub(super) async fn send_stream_status<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    status: TerminalPaneStatus,
    exit_code: Option<u32>,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&TerminalAttachServerMessage::Status { status, exit_code })
        .expect("terminal status serialization cannot fail");
    send_stream(sender, runtime, owner, Message::Text(text.into())).await
}

pub(super) async fn send_stream_error<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    code: &str,
    message: &str,
) -> bool
where
    S: Sink<Message> + Unpin,
{
    let text = socket_error_text(code, message);
    send_stream(sender, runtime, owner, Message::Text(text.into())).await
}

pub(in crate::runtime) enum SocketSendError<E> {
    Sink(E),
    Timeout,
}

pub(super) async fn bounded_send<S>(
    sender: &mut S,
    message: Message,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    match timeout(SOCKET_SEND_TIMEOUT, sender.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(SocketSendError::Sink(error)),
        Err(_) => Err(SocketSendError::Timeout),
    }
}

pub(super) async fn bounded_close<S>(sender: &mut S) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    bounded_close_with_timeout(sender, SOCKET_SEND_TIMEOUT).await
}

pub(in crate::runtime) async fn bounded_close_with_timeout<S>(
    sender: &mut S,
    close_timeout: Duration,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    match timeout(close_timeout, sender.close()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(SocketSendError::Sink(error)),
        Err(_) => Err(SocketSendError::Timeout),
    }
}

pub(super) async fn send_control<S>(
    sender: &mut S,
    state: TerminalControlState,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&TerminalAttachServerMessage::Control { state })
        .expect("terminal control serialization cannot fail");
    bounded_send(sender, Message::Text(text.into())).await
}

pub(super) async fn send_controller_required<S>(
    sender: &mut S,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    send_socket_error(
        sender,
        "controller_required",
        "claim terminal control before sending input or resize",
    )
    .await
}

pub(super) async fn send_attachment_superseded<S>(
    sender: &mut S,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    send_socket_error(
        sender,
        "attachment_superseded",
        "terminal attachment was superseded by a newer controller",
    )
    .await
}

pub(super) async fn send_resized<S>(
    sender: &mut S,
    cols: u16,
    rows: u16,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    let text = serde_json::to_string(&TerminalAttachServerMessage::Resized { cols, rows })
        .expect("terminal resize serialization cannot fail");
    bounded_send(sender, Message::Text(text.into())).await
}

fn socket_error_text(code: &str, message: &str) -> String {
    serde_json::to_string(&TerminalAttachServerMessage::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    })
    .expect("terminal error serialization cannot fail")
}

pub(super) async fn send_socket_error<S>(
    sender: &mut S,
    code: &str,
    message: &str,
) -> Result<(), SocketSendError<S::Error>>
where
    S: Sink<Message> + Unpin,
{
    bounded_send(
        sender,
        Message::Text(socket_error_text(code, message).into()),
    )
    .await
}
