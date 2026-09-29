use super::*;
use serde::Deserialize;

mod files;
mod identity;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum TemporaryNoteExtension {
    Md,
    Txt,
}

impl TemporaryNoteExtension {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Md => "md",
            Self::Txt => "txt",
        }
    }
}

pub(super) use files::{
    CreateTemporaryNoteRequest, TemporaryNoteResult, create_temporary_note_file,
    notes_root_canonical, prepare_notes_directory,
};
pub(super) use identity::default_notes_identity;
#[cfg(test)]
pub(super) use identity::{local_repository_identity, parse_remote_identity, process_account_name};
