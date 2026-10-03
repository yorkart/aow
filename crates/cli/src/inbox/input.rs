use anyhow::Result;
use clap::Args;
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Args)]
pub(super) struct TextInput {
    /// UTF-8 Markdown text (maximum 128 KiB).
    #[arg(long, conflicts_with = "file")]
    content: Option<String>,
    /// Read Markdown from a UTF-8 file, or - for standard input (maximum 128 KiB).
    #[arg(long, conflicts_with = "content", value_name = "PATH")]
    file: Option<PathBuf>,
}

impl TextInput {
    pub fn read(self) -> Result<Option<String>> {
        let text = if let Some(path) = self.file {
            let source: Box<dyn Read> = if path.as_os_str() == "-" {
                Box::new(io::stdin().lock())
            } else {
                Box::new(std::fs::File::open(path)?)
            };
            let mut bytes = Vec::new();
            source.take(128 * 1024 + 1).read_to_end(&mut bytes)?;
            Some(String::from_utf8(bytes).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "Inbox text must be UTF-8")
            })?)
        } else {
            self.content
        };
        if let Some(text) = &text {
            if text.trim().is_empty() || text.len() > 128 * 1024 || text.contains('\0') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Inbox text must contain 1-131072 bytes without NUL",
                )
                .into());
            }
        }
        Ok(text)
    }

    pub fn required(self) -> Result<String> {
        self.read()?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "provide --content or --file").into()
        })
    }
}
