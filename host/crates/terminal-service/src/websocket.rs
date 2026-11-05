//! The network boundary: decode commands and encode typed core events.

use aow_terminal_core::{TerminalConnection, TerminalEvent, TerminalInput};
use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt, future};

pub(crate) async fn runtime_socket(socket: WebSocket, connection: TerminalConnection) {
    let (sender, receiver) = socket.split();
    // Adapt the socket directly. Sink::flush still flushes the actual socket,
    // so core's delivery gate protects claim ACKs and output against takeover.
    let sender = sender.with(|event| future::ready(Ok::<_, axum::Error>(encode(event))));
    let receiver = receiver
        .take_while(|message| {
            future::ready(matches!(message, Ok(message) if !matches!(message, Message::Close(_))))
        })
        .filter_map(|message| future::ready(message.ok().and_then(decode)));
    connection.run(sender, receiver).await;
}

fn decode(message: Message) -> Option<TerminalInput> {
    match message {
        Message::Binary(bytes) => Some(TerminalInput::Data(bytes)),
        Message::Text(text) => Some(match serde_json::from_str(&text) {
            Ok(command) => TerminalInput::Command(command),
            Err(error) => TerminalInput::InvalidCommand(error.to_string()),
        }),
        // Axum/tungstenite handles the WebSocket ping/pong lifecycle.
        Message::Ping(_) | Message::Pong(_) | Message::Close(_) => None,
    }
}

fn encode(event: TerminalEvent) -> Message {
    match event {
        TerminalEvent::Output(bytes) => Message::Binary(bytes),
        TerminalEvent::Message(message) => Message::Text(
            serde_json::to_string(&message)
                .expect("terminal message serialization cannot fail")
                .into(),
        ),
    }
}
