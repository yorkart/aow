//! Prepared adapter launch configuration for transport hosts such as acpd.

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

/// Resolved through the same settings, Registry cache and environment as local ACP.
/// The caller owns the child process and transports ACP messages without translation.
pub struct AdapterCommand {
    pub path: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub execution_path: Option<OsString>,
}
