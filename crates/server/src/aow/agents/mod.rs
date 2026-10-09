mod launch;
mod registry;
#[cfg(test)]
mod tests;

pub(crate) use registry::resolve_executable;
pub(super) use registry::{AgentsQuery, RegisterAgentRequest};
