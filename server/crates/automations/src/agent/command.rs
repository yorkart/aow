use crate::AgentKind;
use anyhow::Result;
use aow_agents::automation::AgentAutomation;
pub fn validate_arguments(kind: AgentKind, arguments: &[String]) -> Result<()> {
    kind.validate_arguments(arguments)
}
