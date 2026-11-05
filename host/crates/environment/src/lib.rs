//! Execution-node HOME, identity import and skill provisioning. No business roles or schedules.
mod error;
mod home;
mod identity;
mod manager;
mod model;
pub use error::EnvironmentError;
pub use home::PreparedHome;
pub use identity::role_layout;
pub use manager::EnvironmentManager;
pub use model::{HomeLayout, Identity, IdentityPreset, PreparedEnvironment, RoleEnvironmentSpec};

#[cfg(test)]
mod tests;
