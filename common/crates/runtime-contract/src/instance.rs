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
