use super::ExecutionEnvironment;
use anyhow::{Result, ensure};

impl ExecutionEnvironment {
    pub(crate) fn validate(&self) -> Result<()> {
        ensure!(
            self.home.is_absolute() && self.home.is_dir(),
            "Execution Home must be an existing absolute directory"
        );
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
