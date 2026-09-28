use std::path::PathBuf;

/// Platform-neutral metadata. A zero tty means no controlling terminal.
#[derive(Debug)]
pub struct ProcessInfo {
    pub pid: i32,
    pub parent: i32,
    pub group: i32,
    pub session: i32,
    pub tty: i32,
    pub foreground: i32,
    /// Unix process state, including Z (zombie) and T/t (stopped).
    pub state: char,
    /// Opaque OS start identity; Linux keeps its existing /proc clock ticks.
    pub start_time: String,
}

/// Entrypoints used only for local recognition. Never log or publish arguments.
#[derive(Default)]
pub struct ProcessCommand {
    pub executable: Option<PathBuf>,
    /// NUL-separated arguments, with their original boundaries preserved.
    pub arguments: Vec<u8>,
}
