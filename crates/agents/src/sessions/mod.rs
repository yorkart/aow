mod claude;
mod codex;
mod codex_like;
mod helpers;
mod hermes;
mod model;
mod provider;
mod roots;
pub mod snapshot;
pub mod tail;
pub mod titles;
pub mod tracking;
mod traecli;

use std::{
    ffi::OsStr,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use crate::{Agent, AgentDefinition, CLAUDE};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde_json::{Map, Value};
use walkdir::WalkDir;

pub(super) const STATE_DB_FILENAME: &str = "state_5.sqlite";
const SQLITE_BUSY_TIMEOUT_MS: u64 = 250;

pub use model::{AgentSession, AgentSessionLocator, AgentSessionProvider, SessionEnvironment};
pub use provider::{SessionAgent, find_session, list_sessions};
pub use roots::SessionRoots;

use helpers::{fallback_title, normalize_path, normalize_title, path_is_inside_or_equal, session};

#[cfg(test)]
mod tests;
