//! Runtime listener identity and advertised capabilities.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostHealth {
    pub service: String,
    pub api_version: u32,
    pub node_id: String,
    pub instance_id: String,
    pub version: String,
    pub pid: u32,
    pub capabilities: Vec<String>,
}
