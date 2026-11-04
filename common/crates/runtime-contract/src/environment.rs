use serde::{Deserialize, Serialize};

/// Caller-prepared process environment, retained for reconnects and session recovery.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionEnvironment {
    /// Existing absolute directory prepared and managed by the caller.
    pub home: std::path::PathBuf,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

pub use ExecutionEnvironment as NodeEnvironment;

#[derive(Debug, Serialize, Deserialize)]
pub struct EnvironmentPreparation {
    pub token: String,
    #[serde(flatten)]
    pub environment: NodeEnvironment,
}

use anyhow::{Result, ensure};

impl ExecutionEnvironment {
    /// Validate caller-prepared node-local directories and process overrides.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.home.is_absolute() && self.home.is_dir(),
            "Execution Home must be an existing absolute directory"
        );
        self.validate_shape()
    }
    pub fn validate_shape(&self) -> Result<()> {
        ensure!(self.home.is_absolute(), "Execution Home must be absolute");
        for (key, value) in &self.env {
            ensure!(
                !key.is_empty() && !key.contains(['=', '\0']) && !value.contains('\0'),
                "Invalid execution environment variable"
            );
            ensure!(key != "HOME", "Set execution Home using the home field");
        }
        Ok(())
    }
}
