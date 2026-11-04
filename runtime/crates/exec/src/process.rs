use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub stdin: Vec<u8>,
    pub timeout_ms: u64,
    pub max_output_bytes: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ProcessResult {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
struct Group(u32);
impl Drop for Group {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.0 as i32), libc::SIGKILL);
        }
    }
}
async fn limited(reader: impl AsyncRead + Unpin, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes).await?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Process output exceeded its limit"
    );
    Ok(bytes)
}
pub async fn run_once(spec: ProcessSpec) -> Result<ProcessResult> {
    ensure!(
        spec.cwd.is_absolute() && spec.executable.is_absolute(),
        "Execution paths must be absolute on the node"
    );
    ensure!(
        (1..=120_000).contains(&spec.timeout_ms),
        "Invalid process timeout"
    );
    ensure!(
        spec.max_output_bytes <= 32 * 1024 * 1024 && spec.stdin.len() <= 8 * 1024 * 1024,
        "Invalid process limits"
    );
    tokio::time::timeout(Duration::from_millis(spec.timeout_ms), async move {
        let mut child = Command::new(spec.executable)
            .args(spec.arguments)
            .current_dir(spec.cwd)
            .envs(spec.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true)
            .spawn()?;
        let group = Group(child.id().unwrap());
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let ((), stdout, stderr, status) = tokio::try_join!(
            async move {
                stdin.write_all(&spec.stdin).await?;
                drop(stdin);
                Ok::<_, anyhow::Error>(())
            },
            limited(stdout, spec.max_output_bytes),
            limited(stderr, spec.max_output_bytes),
            async {
                let status = child.wait().await?;
                drop(group);
                Ok::<_, anyhow::Error>(status)
            }
        )?;
        Ok(ProcessResult {
            exit_code: status.code(),
            success: status.success(),
            stdout,
            stderr,
        })
    })
    .await
    .map_err(|_| anyhow::anyhow!("Node execution timed out"))?
}
