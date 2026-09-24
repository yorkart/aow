//! Validation of terminal identifiers and creation specifications.

use super::*;

pub(super) fn validate_id(id: &str) -> Result<(), TerminaldError> {
    if id.is_empty() {
        return Err(TerminaldError::Invalid(
            "runtime ID cannot be empty".to_owned(),
        ));
    }
    if id.len() > MAX_RUNTIME_ID_BYTES {
        return Err(TerminaldError::Invalid(format!(
            "runtime ID cannot exceed {MAX_RUNTIME_ID_BYTES} bytes"
        )));
    }
    if id.chars().any(char::is_control) {
        return Err(TerminaldError::Invalid(
            "runtime ID cannot contain control characters".to_owned(),
        ));
    }
    Ok(())
}

pub(super) async fn validate_spec(spec: &TerminalRuntimeSpec) -> Result<(), TerminaldError> {
    validate_dimensions(spec.rows, spec.cols)?;
    let cwd = Path::new(&spec.cwd);
    if !cwd.is_absolute() {
        return Err(TerminaldError::Invalid(format!(
            "terminal cwd must be absolute: {}",
            spec.cwd
        )));
    }
    let cwd_metadata = tokio::fs::metadata(cwd).await.map_err(|error| {
        TerminaldError::Invalid(format!("cannot access cwd {}: {error}", spec.cwd))
    })?;
    if !cwd_metadata.is_dir() {
        return Err(TerminaldError::Invalid(format!(
            "terminal cwd is not a directory: {}",
            spec.cwd
        )));
    }

    let shell = Path::new(&spec.shell);
    if !shell.is_absolute() {
        return Err(TerminaldError::Invalid(format!(
            "terminal shell must be absolute: {}",
            spec.shell
        )));
    }
    let shell_metadata = tokio::fs::metadata(shell).await.map_err(|error| {
        TerminaldError::Invalid(format!("cannot access shell {}: {error}", spec.shell))
    })?;
    if !shell_metadata.is_file() || shell_metadata.permissions().mode() & 0o111 == 0 {
        return Err(TerminaldError::Invalid(format!(
            "terminal shell is not an executable file: {}",
            spec.shell
        )));
    }
    if spec.arguments.len() > 128
        || spec
            .arguments
            .iter()
            .any(|argument| argument.len() > 8192 || argument.contains('\0'))
    {
        return Err(TerminaldError::Invalid(
            "terminal arguments are invalid".to_owned(),
        ));
    }
    if spec.environment.len() > 128
        || spec.environment.iter().any(|(key, value)| {
            key.is_empty()
                || key.len() > 256
                || value.len() > 64 * 1024
                || key.contains(['=', '\0'])
                || value.contains('\0')
        })
    {
        return Err(TerminaldError::Invalid(
            "terminal environment is invalid".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_dimensions(rows: u16, cols: u16) -> Result<(), TerminaldError> {
    if !(1..=1000).contains(&rows) || !(1..=1000).contains(&cols) {
        return Err(TerminaldError::Invalid(
            "terminal rows and cols must be between 1 and 1000".to_owned(),
        ));
    }
    Ok(())
}
