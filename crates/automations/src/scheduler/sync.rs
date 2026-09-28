use super::*;

impl Scheduler {
    pub async fn sync(&self, store: &Store, task: &Task) -> Result<()> {
        if task.input.kind == TaskKind::Manual {
            valid_component(&task.id)?;
            task.input.validate_schedule()?;
            // New manual tasks never contact the timer manager. Converting an
            // existing task removes its former trigger, without touching runs.
            let label = self.label(&task.id);
            let extensions = if self.platform == Platform::Systemd {
                vec!["timer", "service"]
            } else {
                vec!["plist"]
            };
            let paths: Vec<_> = extensions
                .iter()
                .map(|extension| self.directory.join(format!("{label}.{extension}")))
                .collect();
            if paths.iter().all(|path| !path.exists()) {
                return Ok(());
            }
            if self.platform == Platform::Systemd {
                if paths[0].exists() {
                    self.command(
                        &self.manager_command,
                        &[
                            "--user".into(),
                            "disable".into(),
                            "--now".into(),
                            format!("{label}.timer"),
                        ],
                    )
                    .await?;
                }
            } else {
                let target = format!("gui/{}/{label}", unsafe { libc::geteuid() });
                if self
                    .command(&self.manager_command, &["print".into(), target.clone()])
                    .await
                    .is_ok()
                {
                    self.command(&self.manager_command, &["bootout".into(), target])
                        .await?;
                }
            }
            for path in paths {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
            if self.platform == Platform::Systemd {
                self.command(
                    &self.manager_command,
                    &["--user".into(), "daemon-reload".into()],
                )
                .await?;
            }
            return Ok(());
        }
        let files = self.render(store, task)?;
        let enabled = task.input.enabled && !task.deleted;
        if enabled {
            self.check_ready().await?;
        } else if files.iter().all(|(path, _)| !path.exists()) {
            return Ok(());
        }
        let label = self.label(&task.id);
        if self.platform == Platform::Systemd {
            // Only the short-lived trigger is replaced; actual runs have separate units.
            let timer = format!("{label}.timer");
            if self.directory.join(&timer).exists() {
                self.command(
                    &self.manager_command,
                    &[
                        "--user".into(),
                        "disable".into(),
                        "--now".into(),
                        timer.clone(),
                    ],
                )
                .await?;
            }
            for (path, content) in files {
                atomic_write(&path, content.as_bytes())?;
            }
            self.command(
                &self.manager_command,
                &["--user".into(), "daemon-reload".into()],
            )
            .await?;
            if enabled {
                self.command(
                    &self.manager_command,
                    &["--user".into(), "enable".into(), "--now".into(), timer],
                )
                .await?;
            }
        } else {
            let domain = format!("gui/{}", unsafe { libc::geteuid() });
            let target = format!("{domain}/{label}");
            if self
                .command(&self.manager_command, &["print".into(), target.clone()])
                .await
                .is_ok()
            {
                self.command(&self.manager_command, &["bootout".into(), target])
                    .await?;
            }
            for (path, content) in files {
                if enabled {
                    atomic_write(&path, content.as_bytes())?;
                    self.command(
                        &self.manager_command,
                        &[
                            "bootstrap".into(),
                            domain.clone(),
                            path.to_string_lossy().into_owned(),
                        ],
                    )
                    .await?;
                } else if path.exists() {
                    fs::remove_file(path)?;
                }
            }
        }
        if task.deleted {
            for (path, _) in self.render(store, task)? {
                if path.exists() {
                    fs::remove_file(path)?;
                }
            }
            if self.platform == Platform::Systemd {
                self.command(
                    &self.manager_command,
                    &["--user".into(), "daemon-reload".into()],
                )
                .await?;
            }
        }
        Ok(())
    }
}
