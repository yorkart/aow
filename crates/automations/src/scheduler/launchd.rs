use super::*;
use std::io::Write;

fn domains() -> [String; 2] {
    let uid = unsafe { libc::geteuid() };
    [format!("gui/{uid}"), format!("user/{uid}")]
}

impl Scheduler {
    pub(super) async fn launchd_domain(&self) -> Result<String> {
        let mut errors = Vec::new();
        for domain in domains() {
            match self
                .command(&self.manager_command, &["print".into(), domain.clone()])
                .await
            {
                Ok(_) => return Ok(domain),
                Err(error) => errors.push(format!("{domain}: {error:#}")),
            }
        }
        anyhow::bail!("无法访问 launchd 用户域：{}", errors.join("；"))
    }

    /// A GUI login can appear or disappear between syncs. Remove the old job
    /// from either user-owned domain before installing its replacement.
    pub(super) async fn bootout_launchd(&self, label: &str) -> Result<()> {
        for domain in domains() {
            let target = format!("{domain}/{label}");
            if self
                .command(&self.manager_command, &["print".into(), target.clone()])
                .await
                .is_ok()
            {
                self.command(
                    &self.manager_command,
                    &["bootout".into(), "--wait".into(), target],
                )
                .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn dispatch_launchd(
        &self,
        store: &Store,
        task: &Task,
        label: &str,
        args: &[String],
        domain: &str,
    ) -> Result<()> {
        // Completed jobs stay registered in launchd. Only retire this task's
        // finished runs; active runners must survive server updates/restarts.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            for run in store.runs(&task.id, None, 100).unwrap_or_default() {
                if run.status.terminal() {
                    let _ = self
                        .bootout_launchd(&format!("{}.run.{}", self.label(&task.id), run.id))
                        .await;
                }
            }
        })
        .await;
        let mut plist = tempfile::Builder::new()
            .prefix("aow-automation-")
            .suffix(".plist")
            .tempfile()?;
        plist.write_all(
            render::launchd_plist(
                label,
                args,
                "<key>RunAtLoad</key><true/>",
                domain.starts_with("user/"),
            )
            .as_bytes(),
        )?;
        // Unlike legacy `submit`, bootstrap targets the chosen user domain even
        // when AoW itself runs as a LaunchDaemon in the privileged system domain.
        // launchd retains the definition after bootstrap; no login-time plist is left.
        self.command(
            &self.dispatch_command,
            &[
                "bootstrap".into(),
                domain.into(),
                plist.path().to_string_lossy().into_owned(),
            ],
        )
        .await?;
        Ok(())
    }
}
