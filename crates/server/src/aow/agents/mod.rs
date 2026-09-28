mod launch;
mod registry;

pub(crate) use registry::resolve_executable;
pub(super) use registry::{AgentsQuery, RegisterAgentRequest};
