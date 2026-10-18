use super::{invalid, preserve_parent};
use crate::{AppState, HttpError};
use aow_protocol::{CreationContext, InstanceRef};

/// Terminal identity is already resolved by the CLI environment. ACP session IDs
/// are connection-scoped and must never be guessed from cwd or a shared process.
pub(crate) fn creation_parent(
    state: &AppState,
    parent: Option<InstanceRef>,
    caller: Option<&CreationContext>,
    no_parent: bool,
) -> Result<Option<InstanceRef>, HttpError> {
    if let Some(caller) = caller {
        caller.validate().map_err(invalid)?;
    }
    if no_parent {
        if parent.is_some() {
            return Err(invalid("parent and no_parent are mutually exclusive"));
        }
        return Ok(None);
    }
    let parent = match (parent, caller) {
        (Some(parent), _) => Some(parent),
        (None, Some(caller)) => {
            let connection_id = caller.acp_connection_id.as_deref().ok_or_else(|| {
                invalid("caller identity is missing: provide AOW_INSTANCE, an ACP connection ID, --parent, or --no-parent")
            })?;
            let id = state
                .acp
                .thread_for_session(connection_id, &caller.session_id)
                .map_err(|error| invalid(format!("cannot resolve ACP caller: {error}")))?;
            Some(InstanceRef::acp(id))
        }
        // Host-originated creation (for example Inbox) has no calling agent session.
        (None, None) => None,
    };
    preserve_parent(state, parent.as_ref())?;
    Ok(parent)
}
