//! Import the runtime's built-in identity files. File contents are opaque.
use super::model::{HomeLayout, Identity, IdentityPreset};
use crate::aow::{AowError, AowManager};
use std::{
    fs::{File, OpenOptions},
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

fn invalid(message: impl Into<String>) -> AowError {
    AowError::Invalid(message.into())
}

fn source_home(source: &str) -> Result<PathBuf, AowError> {
    let path = if source == "~" || source.starts_with("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| invalid("Enter an absolute source Home path"))?;
        home.join(source.strip_prefix("~/").unwrap_or(""))
    } else {
        PathBuf::from(source)
    };
    if !path.is_absolute() || source.contains('\0') {
        return Err(invalid(
            "Source Home must be an absolute path or start with ~/",
        ));
    }
    Ok(path)
}

pub(super) fn validate(identity: &Identity) -> Result<(), AowError> {
    let Identity::LocalHome { source } = identity;
    if let Some(source) = source {
        source_home(source)?;
    }
    Ok(())
}

pub(super) const SUPPORTED_LAYOUTS: &[HomeLayout] =
    &[HomeLayout::Codex, HomeLayout::Claude, HomeLayout::Traecli];

pub(super) fn files(layout: HomeLayout) -> Result<&'static [&'static str], AowError> {
    match layout {
        HomeLayout::Codex => Ok(&[".codex/auth.json"]),
        // File-backed credentials only. OS keychain entries are not exported.
        HomeLayout::Claude => Ok(&[".claude/.credentials.json"]),
        HomeLayout::Traecli => Ok(&[".trae/cli/auth.json"]),
        _ => Err(invalid(
            "This agent does not have a built-in identity preset and is not supported for Agent Roles",
        )),
    }
}

pub(super) fn import(
    identity: &Identity,
    layout: HomeLayout,
    destination: &Path,
) -> Result<(), AowError> {
    validate(identity)?;
    let files = files(layout)?;
    let Identity::LocalHome { source } = identity;
    let Some(source) = source else { return Ok(()) };
    let root = source_home(source)?;
    if !root.is_dir() {
        return Err(invalid(format!(
            "Source Home is not a directory: {}",
            root.display()
        )));
    }
    let mut remaining = 512 * 1024 * 1024_u64;
    for relative in files {
        let input = root.join(relative);
        // Read credential symlinks as independent files so refreshes stay isolated.
        // O_NONBLOCK avoids hanging if a credential path is a FIFO.
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&input)
            .map_err(|error| invalid(format!("Cannot import {}: {error}", input.display())))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(invalid(format!(
                "Identity path must be a regular file: {relative}"
            )));
        }
        if metadata.len() > remaining {
            return Err(invalid("Identity files exceed 512 MiB"));
        }
        let output = destination.join(relative);
        super::home_fixture::private_directory(output.parent().unwrap())?;
        let mut target: File = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&output)?;
        let copied = std::io::copy(&mut file.take(remaining + 1), &mut target)?;
        remaining = remaining
            .checked_sub(copied)
            .ok_or_else(|| invalid("Identity files exceed 512 MiB"))?;
        // Preserve executable permissions while keeping imported files private.
        if metadata.permissions().mode() & 0o111 != 0 {
            target.set_permissions(std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

impl AowManager {
    pub(super) fn role_identity_sources(&self) -> Vec<IdentityPreset> {
        let source = std::env::var_os("HOME")
            .map(|path| PathBuf::from(path).to_string_lossy().into_owned())
            .unwrap_or_else(|| "~".into());
        SUPPORTED_LAYOUTS
            .iter()
            .copied()
            .map(|layout| IdentityPreset {
                layout,
                source: source.clone(),
            })
            .collect()
    }
}
