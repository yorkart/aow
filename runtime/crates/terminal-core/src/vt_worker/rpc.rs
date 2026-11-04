use std::io;

#[derive(Debug)]
pub(super) enum RpcError {
    Io(io::Error),
    Timeout,
    Protocol(String),
    Session(String),
    Worker(String),
    SnapshotTooLarge,
}

pub(super) enum ActorOperation<T> {
    Complete(T),
    Shutdown,
    WorkerExited(String),
}
pub(super) fn format_rpc_error(error: &RpcError) -> String {
    match error {
        RpcError::Io(error) => error.to_string(),
        RpcError::Timeout => "VT worker RPC timed out".to_owned(),
        RpcError::SnapshotTooLarge => "VT snapshot exceeds the byte limit".to_owned(),
        RpcError::Protocol(message) | RpcError::Session(message) | RpcError::Worker(message) => {
            message.clone()
        }
    }
}
