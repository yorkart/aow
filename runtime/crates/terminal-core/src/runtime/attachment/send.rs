use super::*;

pub(in crate::runtime) async fn send_owned<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: ControllerOwner,
    message: TerminalEvent,
) -> bool
where
    S: Sink<TerminalEvent> + Unpin,
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
    message: TerminalEvent,
) -> bool
where
    S: Sink<TerminalEvent> + Unpin,
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
    S: Sink<TerminalEvent> + Unpin,
{
    let message = TerminalAttachServerMessage::Status { status, exit_code };
    send_stream(sender, runtime, owner, TerminalEvent::Message(message)).await
}

pub(super) async fn send_stream_error<S>(
    sender: &mut S,
    runtime: &Runtime,
    owner: Option<ControllerOwner>,
    code: &str,
    message: &str,
) -> bool
where
    S: Sink<TerminalEvent> + Unpin,
{
    let message = attachment_error(code, message);
    send_stream(sender, runtime, owner, TerminalEvent::Message(message)).await
}

pub(in crate::runtime) enum DeliveryError<E> {
    Sink(E),
    Timeout,
}

pub(super) async fn bounded_send<S>(
    sender: &mut S,
    message: TerminalEvent,
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    match timeout(DELIVERY_TIMEOUT, sender.send(message)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(DeliveryError::Sink(error)),
        Err(_) => Err(DeliveryError::Timeout),
    }
}

pub(super) async fn bounded_close<S>(sender: &mut S) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    bounded_close_with_timeout(sender, DELIVERY_TIMEOUT).await
}

pub(in crate::runtime) async fn bounded_close_with_timeout<S>(
    sender: &mut S,
    close_timeout: Duration,
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    match timeout(close_timeout, sender.close()).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(DeliveryError::Sink(error)),
        Err(_) => Err(DeliveryError::Timeout),
    }
}

pub(super) async fn send_control<S>(
    sender: &mut S,
    state: TerminalControlState,
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    bounded_send(
        sender,
        TerminalEvent::Message(TerminalAttachServerMessage::Control { state }),
    )
    .await
}

pub(super) async fn send_controller_required<S>(
    sender: &mut S,
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    send_attachment_error(
        sender,
        "controller_required",
        "claim terminal control before sending input or resize",
    )
    .await
}

pub(super) async fn send_attachment_superseded<S>(
    sender: &mut S,
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    send_attachment_error(
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
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    bounded_send(
        sender,
        TerminalEvent::Message(TerminalAttachServerMessage::Resized { cols, rows }),
    )
    .await
}

fn attachment_error(code: &str, message: &str) -> TerminalAttachServerMessage {
    TerminalAttachServerMessage::Error {
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

pub(super) async fn send_attachment_error<S>(
    sender: &mut S,
    code: &str,
    message: &str,
) -> Result<(), DeliveryError<S::Error>>
where
    S: Sink<TerminalEvent> + Unpin,
{
    bounded_send(
        sender,
        TerminalEvent::Message(attachment_error(code, message)),
    )
    .await
}
