use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(super) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| crate::sessions::helpers::normalize_path(path))
}

// Like SessionRoots::codex, root is CODEX_HOME, not its sessions subdirectory.
pub(super) fn thread_ids(root: &Path, files: &[aow_process::OpenFile]) -> BTreeSet<String> {
    let locks = canonical(&root.join("thread-writer-locks"));
    let sessions = canonical(&root.join("sessions"));
    let mut ids = BTreeSet::new();
    for file in files.iter().filter(|file| file.writable) {
        if !file
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.ends_with(".lock")
                    || (name.starts_with("rollout-") && name.ends_with(".jsonl"))
            })
        {
            continue;
        }
        let path = canonical(&file.path);
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let id = if path.parent() == Some(locks.as_path()) {
            name.strip_suffix(".lock")
        } else if path.starts_with(&sessions) {
            rollout_thread_id(name)
        } else {
            None
        };
        if let Some(id) = id.filter(|id| valid_id(id)) {
            ids.insert(id.to_owned());
        }
    }
    ids
}

fn rollout_thread_id(name: &str) -> Option<&str> {
    let core = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    if core.get(19..20)? != "-" {
        return None;
    }
    let ids = core.get(20..)?;
    // Revert adds a distinct rollout UUID after the stable thread UUID.
    Some(ids.split_once('_').map_or(ids, |(thread, _)| thread))
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}
