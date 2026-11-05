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

pub(in crate::projects) use files::{
    CreateTemporaryNoteRequest, TemporaryNoteResult, notes_root_canonical,
};
pub(crate) use identity::default_notes_identity;
#[cfg(test)]
pub(crate) use identity::{local_repository_identity, parse_remote_identity};
