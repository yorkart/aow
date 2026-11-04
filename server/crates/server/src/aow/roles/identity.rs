//! Identity policy; source expansion and credential reads happen on the node.
use super::model::{HomeLayout, Identity};
use crate::aow::AowError;
fn invalid(message: impl Into<String>) -> AowError {
    AowError::Invalid(message.into())
}
pub(super) fn validate(identity: &Identity) -> Result<(), AowError> {
    let Identity::LocalHome { source } = identity;
    if let Some(source) = source
        && (source.contains('\0')
            || !(std::path::Path::new(source).is_absolute()
                || source == "~"
                || source.starts_with("~/")))
    {
        return Err(invalid(
            "Source Home must be absolute on the node or start with ~/",
        ));
    }
    Ok(())
}
pub(super) const SUPPORTED_LAYOUTS: &[HomeLayout] =
    &[HomeLayout::Codex, HomeLayout::Claude, HomeLayout::Traecli];

pub(super) fn files(layout: HomeLayout) -> Result<&'static [&'static str], AowError> {
    match layout {
        HomeLayout::Codex => Ok(&[".codex/auth.json"]),
        // File-backed credentials only. OS keychain entries are not exported.
        HomeLayout::Claude => Ok(&[".claude/.credentials.json"]),
        HomeLayout::Traecli => Ok(&[".trae/cli/auth.json"]),
        _ => Err(invalid(
            "This agent does not have a built-in identity preset and is not supported for Agent Roles",
        )),
    }
}
