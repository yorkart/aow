//! Built-in Agent adapters, with capability traits and enum dispatch.
//! Process recognition has no dependencies; native sessions and automation are optional features.

#[cfg(feature = "automation")]
pub mod automation;
#[cfg(any(feature = "launch", feature = "automation"))]
pub mod environment;
#[cfg(feature = "launch")]
pub mod launch;
#[cfg(feature = "sessions")]
pub mod sessions;

pub mod interactive;
pub mod process;
mod registry;

pub use process::{AgentProcessMatcher, ProcessInfo, recognize_process};
pub use registry::{Agent, AgentDefinition, CLAUDE, CODEX, HERMES, KNOWN_AGENTS, TRAECLI};
