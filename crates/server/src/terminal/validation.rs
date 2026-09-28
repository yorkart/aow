use super::*;

pub(super) fn resolve_cwd(
    cwd: Option<&str>,
    workspace_root: Option<&str>,
) -> Result<String, TerminalError> {
    let value = cwd
        .filter(|value| !value.trim().is_empty())
        .or_else(|| workspace_root.filter(|value| !value.trim().is_empty()))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("/"))
                .to_string_lossy()
                .into_owned()
        });
    if !Path::new(&value).is_absolute() {
        return Err(TerminalError::Invalid(format!(
            "terminal cwd must be absolute: {value}"
        )));
    }
    Ok(value)
}

pub(super) async fn validate_directory(path: &str) -> Result<(), TerminalError> {
    if !Path::new(path).is_absolute() {
        return Err(TerminalError::Invalid(format!(
            "directory must be absolute: {path}"
        )));
    }
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| TerminalError::Invalid(format!("cannot access {path}: {error}")))?;
    if !metadata.is_dir() {
        return Err(TerminalError::Invalid(format!(
            "path is not a directory: {path}"
        )));
    }
    Ok(())
}

pub(super) fn normalize_shell(shell: Option<&str>) -> Result<String, TerminalError> {
    let shell = shell
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            std::env::var("SHELL")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| {
            if Path::new("/bin/bash").is_file() {
                "/bin/bash".to_owned()
            } else {
                "/bin/sh".to_owned()
            }
        });
    if shell.as_bytes().contains(&0) {
        return Err(TerminalError::Invalid(
            "shell contains a NUL byte".to_owned(),
        ));
    }
    Ok(shell)
}

pub(super) fn terminal_size(
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<(u16, u16), TerminalError> {
    let rows = rows.unwrap_or(DEFAULT_ROWS);
    let cols = cols.unwrap_or(DEFAULT_COLS);
    validate_dimensions(rows, cols)?;
    Ok((rows, cols))
}

fn validate_dimensions(rows: u16, cols: u16) -> Result<(), TerminalError> {
    if !(1..=1000).contains(&rows) || !(1..=1000).contains(&cols) {
        return Err(TerminalError::Invalid(
            "terminal rows and cols must be between 1 and 1000".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_name(name: &str) -> Result<String, TerminalError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TerminalError::Invalid(
            "terminal name cannot be empty".to_owned(),
        ));
    }
    if name.chars().count() > 128 {
        return Err(TerminalError::Invalid(
            "terminal name cannot exceed 128 characters".to_owned(),
        ));
    }
    Ok(name.to_owned())
}

pub(super) fn default_pane_name(cwd: &str, shell: &str) -> String {
    let name = Path::new(cwd)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .or_else(|| (!cwd.trim().is_empty()).then_some(cwd.trim()))
        .or_else(|| {
            Path::new(shell)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or("Shell");
    name.chars().take(128).collect()
}

pub(super) fn migrate_terminal_names(tab: &mut TerminalTab) {
    // Old metadata did not distinguish generated names from explicit renames.
    // Recognize only the old defaults; preserve all other labels. New renames
    // carry an explicit flag, even when the user chooses a default-looking name.
    let is_agent_default = |name: &str, pane: &TerminalPane| {
        pane.agent_id
            .as_deref()
            .and_then(aow_agents::Agent::from_id)
            .is_some_and(|agent| agent.definition().display_name == name)
    };
    if tab.name_is_custom.is_none() {
        let numbered = tab
            .name
            .strip_prefix("Terminal ")
            .is_some_and(|suffix| suffix.parse::<usize>().is_ok());
        let generated = numbered
            || tab
                .panes
                .iter()
                .any(|pane| is_agent_default(&tab.name, pane));
        tab.name_is_custom = Some(!generated);
    }
}
