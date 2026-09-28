use super::*;

pub(super) fn map_client_error(error: TerminaldClientError) -> TerminalError {
    match &error {
        TerminaldClientError::Io(_)
        | TerminaldClientError::Hyper(_)
        | TerminaldClientError::WebSocket(_) => TerminalError::DaemonUnavailable(error.to_string()),
        TerminaldClientError::HttpStatus { status, .. } if *status == StatusCode::CONFLICT => {
            TerminalError::Conflict(error.to_string())
        }
        _ => TerminalError::Daemon(error.to_string()),
    }
}

pub(super) fn map_create_error(error: TerminaldClientError) -> TerminalError {
    match map_client_error(error) {
        TerminalError::Daemon(message) => TerminalError::RuntimeCreate(message),
        error => error,
    }
}

pub(super) fn client_error_is_not_found(error: &TerminaldClientError) -> bool {
    matches!(
        error,
        TerminaldClientError::HttpStatus { status, .. } if *status == StatusCode::NOT_FOUND
    ) || matches!(
        error,
        TerminaldClientError::WebSocket(tungstenite::Error::Http(response))
            if response.status() == StatusCode::NOT_FOUND
    )
}

pub(super) fn runtime_spec(pane: &TerminalPane) -> TerminalRuntimeSpec {
    TerminalRuntimeSpec {
        cwd: pane.cwd.clone(),
        shell: pane.shell.clone(),
        arguments: pane.arguments.clone(),
        environment: Default::default(),
        rows: pane.rows,
        cols: pane.cols,
    }
}

pub(super) fn apply_runtime(pane: &mut TerminalPane, runtime: &TerminalRuntime) -> bool {
    if pane.status == runtime.status
        && pane.exit_code == runtime.exit_code
        && pane.rows == runtime.rows
        && pane.cols == runtime.cols
    {
        return false;
    }
    pane.status = runtime.status;
    pane.exit_code = runtime.exit_code;
    pane.rows = runtime.rows;
    pane.cols = runtime.cols;
    pane.updated_at = timestamp();
    true
}
