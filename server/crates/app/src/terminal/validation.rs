use super::*;

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
            aow_i18n::text("validation.terminal_rows_and_cols_must_be_between_1_and_1000")
                .to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_name(name: &str) -> Result<String, TerminalError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(TerminalError::Invalid(
            aow_i18n::text("validation.terminal_name_cannot_be_empty").to_owned(),
        ));
    }
    if name.chars().count() > 128 {
        return Err(TerminalError::Invalid(
            aow_i18n::text("validation.terminal_name_cannot_exceed_128_characters").to_owned(),
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
    if tab.name_is_custom.is_none() {
        let numbered = tab
            .name
            .strip_prefix("Terminal ")
            .is_some_and(|suffix| suffix.parse::<usize>().is_ok());
        tab.name_is_custom = Some(!numbered);
    }
}

impl TerminalManager {
    pub(super) async fn validate_node_directory(&self, path: &str) -> Result<(), TerminalError> {
        let metadata = self
            .inner
            .terminald
            .runtime()
            .workspace_metadata(Path::new(path), true)
            .await
            .map_err(map_client_error)?;
        if !metadata.directory {
            return Err(TerminalError::Invalid(format!(
                "Not a directory on the node: {path}"
            )));
        }
        Ok(())
    }
    pub(super) async fn node_cwd(
        &self,
        cwd: Option<&str>,
        workspace_root: Option<&str>,
    ) -> Result<String, TerminalError> {
        if let Some(value) = cwd
            .filter(|value| !value.trim().is_empty())
            .or_else(|| workspace_root.filter(|value| !value.trim().is_empty()))
        {
            if !Path::new(value).is_absolute() {
                return Err(TerminalError::Invalid("Node cwd must be absolute".into()));
            }
            return Ok(value.into());
        }
        Ok(self
            .inner
            .terminald
            .runtime()
            .workspace_home()
            .await
            .map_err(map_client_error)?
            .to_string_lossy()
            .into_owned())
    }
    pub(super) async fn node_shell(&self, shell: Option<&str>) -> Result<String, TerminalError> {
        let shell = if let Some(shell) = shell.filter(|shell| !shell.trim().is_empty()) {
            shell.to_owned()
        } else {
            self.inner
                .terminald
                .runtime()
                .workspace_terminal_defaults()
                .await
                .map_err(map_client_error)?
        };
        if shell.contains('\0') {
            return Err(TerminalError::Invalid("Shell contains a NUL byte".into()));
        }
        Ok(shell)
    }
}
