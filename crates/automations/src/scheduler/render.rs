use super::*;

pub(super) fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub(super) fn unit_arg(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

impl Scheduler {
    pub fn render(&self, store: &Store, task: &Task) -> Result<Vec<(PathBuf, String)>> {
        valid_component(&task.id)?;
        task.input.validate_schedule()?;
        if task.input.kind == TaskKind::Manual {
            return Ok(Vec::new());
        }
        let label = self.label(&task.id);
        let args = self.arguments(store, task, "trigger", RunSource::Scheduled);
        match self.platform {
            Platform::Systemd => {
                let service = format!(
                    "[Unit]\nDescription=AoW automation trigger {}\n[Service]\nType=oneshot\nExecStart=:{}\nUMask=0077\nStandardOutput=null\nStandardError=journal\n",
                    task.id,
                    args.iter()
                        .map(|arg| unit_arg(arg))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                let timer = format!(
                    "[Unit]\nDescription=AoW automation {}\n[Timer]\n{}\nPersistent=false\nAccuracySec=1s\n[Install]\nWantedBy=timers.target\n",
                    task.id,
                    if let Some(seconds) = task.input.interval_seconds {
                        format!("OnActiveSec={seconds}s\nOnUnitInactiveSec={seconds}s")
                    } else {
                        Schedule::parse(&task.input.cron)?
                            .systemd_calendars()
                            .iter()
                            .map(|v| format!("OnCalendar={v}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                );
                Ok(vec![
                    (self.directory.join(format!("{label}.service")), service),
                    (self.directory.join(format!("{label}.timer")), timer),
                ])
            }
            Platform::Launchd => {
                let plist = format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array>{}</array>{}<key>RunAtLoad</key><false/><key>KeepAlive</key><false/><key>StandardOutPath</key><string>/dev/null</string><key>StandardErrorPath</key><string>/dev/null</string></dict></plist>\n",
                    args.iter()
                        .map(|arg| format!("<string>{}</string>", xml(arg)))
                        .collect::<String>(),
                    if let Some(seconds) = task.input.interval_seconds {
                        format!(
                            "<key>StartInterval</key><integer>{seconds}</integer><key>ThrottleInterval</key><integer>1</integer>"
                        )
                    } else {
                        format!(
                            "<key>StartCalendarInterval</key>{}",
                            Schedule::parse(&task.input.cron)?.launchd_calendar_xml()?
                        )
                    }
                );
                Ok(vec![(self.directory.join(format!("{label}.plist")), plist)])
            }
            Platform::Unsupported => anyhow::bail!("不支持的系统定时器"),
        }
    }
}
