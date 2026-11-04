use super::ExecutionEnvironment;
use anyhow::{Result, ensure};

pub trait ValidateExecution {
    fn validate_shape(&self) -> Result<()>;
    fn validate(&self) -> Result<()>;
}
impl ValidateExecution for ExecutionEnvironment {
    /// Validate caller-prepared node-local directories and process overrides.
    fn validate(&self) -> Result<()> {
        ensure!(
            self.home.is_absolute() && self.home.is_dir(),
            "Execution Home must be an existing absolute directory"
        );
        self.validate_shape()
    }
    fn validate_shape(&self) -> Result<()> {
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
