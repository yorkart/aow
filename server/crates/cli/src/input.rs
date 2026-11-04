use anyhow::Result;
use clap::Args;
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Args)]
pub(crate) struct CreationArgs {
    /// Caller's native session ID (or AOW_SESSION_ID); never the child session ID.
    #[arg(long)]
    session_id: Option<String>,
    /// Scope an ACP caller session to a connection (or AOW_ACP_CONNECTION_ID).
    #[arg(long)]
    acp_connection_id: Option<String>,
    /// Creating instance: terminal:PANE-ID or acp:THREAD-ID. Overrides inherited identity.
    #[arg(long, conflicts_with = "no_parent")]
    parent: Option<aow_protocol::InstanceRef>,
    /// Create a root instance even inside another AoW instance.
    #[arg(long)]
    no_parent: bool,
}

pub(crate) struct ResolvedCreation {
    pub parent: Option<aow_protocol::InstanceRef>,
    pub caller: aow_protocol::CreationContext,
    pub no_parent: bool,
}

impl CreationArgs {
    pub(crate) fn resolve(self) -> Result<ResolvedCreation> {
        use aow_protocol::InstanceRef;
        let caller = aow_protocol::CreationContext {
            session_id: input_or_environment(self.session_id, "AOW_SESSION_ID")?.ok_or_else(
                || {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "create requires --session-id or AOW_SESSION_ID for the calling session",
                    )
                },
            )?,
            acp_connection_id: input_or_environment(
                self.acp_connection_id,
                "AOW_ACP_CONNECTION_ID",
            )?,
        };
        caller
            .validate()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let parent = if self.no_parent {
            None
        } else if let Some(parent) = self.parent {
            Some(parent)
        } else if let Some(value) = std::env::var_os("AOW_INSTANCE") {
            Some(
                value
                    .into_string()
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "AOW_INSTANCE is not UTF-8")
                    })?
                    .parse::<InstanceRef>()
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?,
            )
        } else {
            None
        };
        if let Some(parent) = &parent {
            parent
                .validate()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        }
        Ok(ResolvedCreation {
            parent,
            caller,
            no_parent: self.no_parent,
        })
    }
}

fn input_or_environment(value: Option<String>, name: &str) -> Result<Option<String>> {
    if value.is_some() {
        return Ok(value);
    }
    std::env::var_os(name)
        .map(|value| {
            value.into_string().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("{name} is not UTF-8")).into()
            })
        })
        .transpose()
}

#[derive(Args)]
pub(crate) struct TaskInput {
    #[arg(long, conflicts_with = "task_file")]
    task: Option<String>,
    /// Read UTF-8 task text from a file, or '-' for stdin (maximum 128 KiB).
    #[arg(long, conflicts_with = "task")]
    task_file: Option<PathBuf>,
}

impl TaskInput {
    pub(crate) fn read(self) -> Result<Option<String>> {
        let Some(path) = self.task_file else {
            return Ok(self.task);
        };
        let mut text = String::new();
        if path.as_os_str() == "-" {
            io::stdin().take(131_073).read_to_string(&mut text)?;
        } else {
            std::fs::File::open(path)?
                .take(131_073)
                .read_to_string(&mut text)?;
        }
        if text.len() > 131_072 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "task exceeds 128 KiB").into());
        }
        Ok(Some(text))
    }
}
