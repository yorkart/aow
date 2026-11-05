mod error;
pub(crate) mod events;
pub(crate) mod execution;
mod host;
mod model;
mod service;

pub use error::is_auth_required;
pub use events::{SessionVersion, SyncEvent, SyncResource};
pub use host::AcpHost;
pub use model::*;
pub use service::AcpService;
mod adapter;
pub use adapter::AdapterCommand;

mod transport;
pub use transport::{AcpConnector, AdapterTransport};
