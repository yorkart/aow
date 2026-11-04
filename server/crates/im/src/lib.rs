//! IM transport implementations. Notification dispatch only sees this interface.

mod config;
mod feishu;
mod message;
mod provider;
pub mod wechat;

pub use config::{ImConfig, ImConfigUpdate, ImConfigView, ImKind};
pub use message::{Field, Message};
pub use provider::{ImProvider, Provider};
