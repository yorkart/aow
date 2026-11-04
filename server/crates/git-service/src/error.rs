use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("{translated}", translated = aow_i18n::message("errors.path_must_be_absolute", &[("p0", .0.to_string())]))]
    PathNotAbsolute(String),
    #[error("{translated}", translated = aow_i18n::message("errors.path_is_not_inside_a_git_repository", &[("p0", .0.to_string())]))]
    NotRepository(String),
    #[error("{translated}", translated = aow_i18n::text("errors.git_command_timed_out"))]
    Timeout,
    #[error("{translated}", translated = aow_i18n::message("errors.git_command_failed", &[("p0", .0.to_string())]))]
    Command(String),
    #[error("{translated}", translated = aow_i18n::text("errors.git_output_was_not_valid_utf"))]
    InvalidUtf8,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    WalkDir(#[from] walkdir::Error),
}
