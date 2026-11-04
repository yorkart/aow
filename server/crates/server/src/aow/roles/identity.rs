//! Identity policy; source expansion and credential reads happen on the node.
use super::model::Identity;
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
