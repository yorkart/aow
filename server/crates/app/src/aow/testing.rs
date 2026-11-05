//! Fixture setup only. Business paths always use the external Runtime.
use super::*;
pub(super) async fn git_output(cwd: &Path, args: &[&str]) -> Result<String, AowError> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .kill_on_drop(true)
        .output()
        .await?;
    if !output.status.success() {
        return Err(AowError::Git(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| AowError::Git(e.to_string()))
}
