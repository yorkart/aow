//! Remote conversation SDK. All session state, adapters and persistence live on the node.
mod client;
mod events;
#[cfg(feature = "test-support")]
pub(crate) mod fixture;
mod methods;
mod native;
pub use crate::is_unavailable;
pub use aow_runtime_contract::acp::*;
pub use client::{AcpClient, AcpObserver};
pub fn is_auth_required(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<client::RemoteOperationError>()
            .is_some_and(|error| error.0.code == "auth_required")
    })
}
