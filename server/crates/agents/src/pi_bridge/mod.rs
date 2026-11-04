//! A per-launch binding and native completion events for interactive Pi sessions.

use std::{
    collections::BTreeMap,
    fs,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
};

const SOURCE: &str = include_str!("extension.mjs");

fn is_bridge(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.strip_prefix("bridge-")
                .and_then(|name| name.strip_suffix(".mjs"))
                .is_some_and(|hash| !hash.is_empty() && hash.chars().all(|c| c.is_ascii_hexdigit()))
        })
}

/// Recover the per-pane identity path from AoW's persisted launch arguments.
pub fn binding_path(args: &[String], launch_id: &str) -> Option<PathBuf> {
    let end = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    args[..end].windows(2).rev().find_map(|pair| {
        let path = Path::new(&pair[1]);
        (pair[0] == "--extension" && is_bridge(path))
            .then(|| {
                path.parent()
                    .map(|directory| directory.join(format!("{launch_id}.json")))
            })
            .flatten()
    })
}

pub fn prepare(
    directory: &Path,
    launch_id: &str,
    args: &mut Vec<String>,
    env: &mut BTreeMap<String, String>,
) -> std::io::Result<()> {
    fs::create_dir_all(directory)?;
    // Rebuilds retain the saved command. Replace only our own injected module
    // so the new pane gets one listener and its own binding identity.
    let mut index = 0;
    while index + 1 < args.len() && args[index] != "--" {
        let path = Path::new(&args[index + 1]);
        if args[index] == "--extension" && path.parent() == Some(directory) && is_bridge(path) {
            args.drain(index..index + 2);
        } else {
            index += 1;
        }
    }
    let mut hash = DefaultHasher::new();
    SOURCE.hash(&mut hash);
    let extension = directory.join(format!("bridge-{:x}.mjs", hash.finish()));
    // Rename a complete file so concurrent launches never load a partial module.
    if !extension.exists() {
        let temporary = directory.join(format!("{launch_id}.mjs.tmp"));
        fs::write(&temporary, SOURCE)?;
        fs::rename(&temporary, &extension)?;
    }
    let insertion = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    args.splice(
        insertion..insertion,
        [
            "--extension".into(),
            extension.to_string_lossy().into_owned(),
        ],
    );
    env.insert(
        "AOW_PI_BINDING".into(),
        directory
            .join(format!("{launch_id}.json"))
            .to_string_lossy()
            .into_owned(),
    );
    Ok(())
}
