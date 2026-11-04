use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum InstanceKind {
    Terminal,
    Acp,
}

/// Stable AoW identity: a terminal pane or a local ACP thread, never a provider session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct InstanceRef {
    pub kind: InstanceKind,
    pub id: String,
}

impl InstanceRef {
    pub fn terminal(id: impl Into<String>) -> Self {
        Self {
            kind: InstanceKind::Terminal,
            id: id.into(),
        }
    }

    pub fn acp(id: impl Into<String>) -> Self {
        Self {
            kind: InstanceKind::Acp,
            id: id.into(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 96
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        {
            return Err("instance ID must contain 1-96 letters, numbers, '-' or '_'".into());
        }
        Ok(())
    }
}

impl std::fmt::Display for InstanceRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self.kind {
            InstanceKind::Terminal => "terminal",
            InstanceKind::Acp => "acp",
        };
        write!(f, "{kind}:{}", self.id)
    }
}

impl std::str::FromStr for InstanceRef {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (kind, id) = value
            .split_once(':')
            .ok_or("expected terminal:ID or acp:ID")?;
        let reference = match kind {
            "terminal" => Self::terminal(id),
            "acp" => Self::acp(id),
            _ => return Err("expected terminal:ID or acp:ID".into()),
        };
        reference.validate()?;
        Ok(reference)
    }
}

/// Execution context of the caller, independent of the newly created instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreationContext {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acp_connection_id: Option<String>,
}

impl CreationContext {
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.trim().is_empty()
            || self.session_id.len() > 1024
            || self.session_id.chars().any(char::is_control)
        {
            return Err(
                "caller session ID must contain 1-1024 bytes without control characters".into(),
            );
        }
        if let Some(id) = &self.acp_connection_id {
            InstanceRef::acp(id).validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcpCreate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller: Option<CreationContext>,
    #[serde(default)]
    pub no_parent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub project_id: String,
    pub cwd: String,
    #[serde(default)]
    pub parent: Option<InstanceRef>,
    pub task: Option<String>,
    pub timeout_seconds: u64,
}

/// Creation metadata retained after the live instance has been deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    #[serde(flatten)]
    pub instance: InstanceRef,
    pub parent: Option<InstanceRef>,
    pub title: String,
    pub cwd: String,
    pub agent_id: Option<String>,
    #[serde(default)]
    pub tab_id: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub deleted: bool,
}
