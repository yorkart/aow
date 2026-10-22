use super::model::{Comment, Document, Item, Label, Run, RunPhase};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

// Keep list metadata alongside labels so each requirement has only two files.
#[derive(Serialize, Deserialize)]
struct Registry {
    version: u32,
    revision: u64,
    labels: Vec<Label>,
    order: Vec<String>,
    captures: BTreeMap<String, (String, String)>,
}

pub(super) struct Persistence {
    pub config: aow_config::ConfigRepository,
    pub runs: PathBuf,
}
impl Persistence {
    pub fn open(state: &Path) -> Result<(Self, Document, Vec<Run>)> {
        let config = aow_config::ConfigRepository::initialize(state)?;
        let path = state
            .join("inbox")
            .join(config.selection().config_id)
            .join("executions.json");
        let persistence = Self { config, runs: path };
        let document = if persistence
            .config
            .directory()
            .join("inbox/labels.json")
            .try_exists()?
        {
            persistence.document()?
        } else {
            let document = Document::default();
            persistence.save_document(&document)?;
            document
        };
        let mut runs: Vec<Run> = read(&persistence.runs)?;
        let mut changed = false;
        for run in &mut runs {
            if run.phase == RunPhase::Starting {
                run.phase = RunPhase::Interrupted;
                run.error =
                    Some(aow_i18n::text("server.service_restarted_execution_result_is_unconfirmed_check_the_terminal").into());
                changed = true;
            }
        }
        if changed {
            write(&persistence.runs, &runs)?;
        }
        Ok((persistence, document, runs))
    }

    pub fn document(&self) -> Result<Document> {
        let root = self.config.directory().join("inbox");
        ensure!(
            !root.is_symlink() && !root.join("requirements").is_symlink(),
            aow_i18n::text("inbox.inbox_directory_cannot_be_a_symlink")
        );
        let registry: Registry = read_required(&root.join("labels.json"))?;
        ensure!(
            registry.version == 1,
            aow_i18n::text("inbox.unsupported_inbox_data_version")
        );
        let mut items = BTreeMap::new();
        for directory in requirement_directories(&root.join("requirements/active"))? {
            let item: Item = read_required(&directory.join("requirement.json"))?;
            ensure!(
                directory.file_name().and_then(|s| s.to_str()) == Some(item.id.as_str()),
                aow_i18n::text("inbox.inbox_directory_and_requirement_id_differ")
            );
            items.insert(item.id.clone(), item);
        }
        let mut ordered = Vec::new();
        let mut seen = HashSet::new();
        for id in registry.order {
            ensure!(
                seen.insert(id.clone()),
                aow_i18n::text("inbox.duplicate_inbox_order_id")
            );
            // Directory moves determine deletion; tolerate a stale order after interruption.
            if let Some(item) = items.remove(&id) {
                ordered.push(item);
            }
        }
        ordered.extend(items.into_values());
        let document = Document {
            version: registry.version,
            revision: registry.revision,
            labels: registry.labels,
            items: ordered,
            captures: registry.captures,
        };
        super::store::validate_document(&document).map_err(|e| anyhow::anyhow!(e.body.message))?;
        Ok(document)
    }

    pub fn save_document(&self, document: &Document) -> Result<()> {
        self.config
            .update_directory(Path::new("inbox"), |root| save_document(root, document))
    }

    pub fn comments(&self, id: &str) -> Result<Vec<Comment>> {
        read_comments(
            &self
                .config
                .directory()
                .join("inbox/requirements/active")
                .join(id)
                .join("comments.jsonl"),
        )
    }

    pub fn append_comment(&self, id: &str, comment: &Comment) -> Result<()> {
        self.config.update_directory(Path::new("inbox"), |root| {
            let directory = root.join("requirements/active").join(id);
            ensure!(
                directory.join("requirement.json").is_file(),
                aow_i18n::text("inbox.requirement_no_longer_exists")
            );
            let path = directory.join("comments.jsonl");
            let previous = read_comments(&path)?;
            // A durable append followed by a failed Git commit is retried without duplication.
            if previous.iter().any(|c| c.id == comment.id) {
                return Ok(());
            }
            let mut bytes = serde_json::to_vec(comment)?;
            bytes.push(b'\n');
            if fs::read(&path)?.last().is_some_and(|byte| *byte != b'\n') {
                bytes.insert(0, b'\n');
            }
            let mut file = fs::OpenOptions::new()
                .append(true)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&path)?;
            let length = file.metadata()?.len();
            if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
                file.set_len(length)?;
                file.sync_all()?;
                return Err(error.into());
            }
            Ok(())
        })
    }
}

fn requirement_directories(path: &Path) -> Result<Vec<PathBuf>> {
    let mut directories = Vec::new();
    if !path.try_exists()? {
        return Ok(directories);
    }
    ensure!(
        !path.is_symlink(),
        aow_i18n::text("inbox.inbox_directory_cannot_be_a_symlink")
    );
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        ensure!(
            entry.file_type()?.is_dir()
                && entry.file_name().to_str().is_some_and(aow_id::is_valid_id),
            aow_i18n::message(
                "inbox.invalid_requirement_directory",
                &[("path", entry.path().display().to_string())]
            )
        );
        directories.push(entry.path());
    }
    Ok(directories)
}

// Roll back filesystem errors across files; Git failures occur after this succeeds
// and intentionally preserve the durable data, matching ConfigRepository::save.
enum Undo {
    File(PathBuf, Option<Vec<u8>>),
    Directory(PathBuf),
    Move(PathBuf, PathBuf),
}
fn save_document(root: &Path, document: &Document) -> Result<()> {
    let active = root.join("requirements/active");
    let deleted = root.join("requirements/deleted");
    for path in [&active, &deleted] {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)?;
    }
    let mut undo = Vec::new();
    let result = (|| {
        let ids: HashSet<_> = document.items.iter().map(|item| item.id.as_str()).collect();
        for directory in requirement_directories(&active)? {
            let id = directory.file_name().unwrap().to_str().unwrap();
            if !ids.contains(id) {
                let destination = deleted.join(id);
                ensure!(
                    !destination.try_exists()?,
                    aow_i18n::message("inbox.deleted_requirement_exists", &[("id", id.to_owned())])
                );
                fs::rename(&directory, &destination)?;
                undo.push(Undo::Move(destination, directory));
            }
        }
        for item in &document.items {
            let directory = active.join(&item.id);
            if !directory.try_exists()? {
                ensure!(
                    !deleted.join(&item.id).try_exists()?,
                    aow_i18n::text("inbox.requirement_is_already_deleted")
                );
                fs::DirBuilder::new().mode(0o700).create(&directory)?;
                undo.push(Undo::Directory(directory.clone()));
            }
            replace(
                &directory.join("requirement.json"),
                &serde_json::to_vec_pretty(item)?,
                &mut undo,
            )?;
            let comments = directory.join("comments.jsonl");
            if !comments.try_exists()? {
                replace(&comments, b"", &mut undo)?;
            }
        }
        let registry = Registry {
            version: document.version,
            revision: document.revision,
            labels: document.labels.clone(),
            order: document.items.iter().map(|item| item.id.clone()).collect(),
            captures: document.captures.clone(),
        };
        replace(
            &root.join("labels.json"),
            &serde_json::to_vec_pretty(&registry)?,
            &mut undo,
        )?;
        fs::File::open(&active)?.sync_all()?;
        fs::File::open(&deleted)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        for operation in undo.into_iter().rev() {
            match operation {
                Undo::File(path, Some(bytes)) => write_bytes(&path, &bytes)?,
                Undo::File(path, None) => match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
                Undo::Directory(path) => fs::remove_dir(path)?,
                Undo::Move(source, destination) => fs::rename(source, destination)?,
            }
        }
    }
    result
}
fn replace(path: &Path, bytes: &[u8], undo: &mut Vec<Undo>) -> Result<()> {
    let previous = match fs::read(path) {
        Ok(previous) if previous == bytes => return Ok(()),
        Ok(previous) => Some(previous),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    undo.push(Undo::File(path.to_path_buf(), previous));
    write_bytes(path, bytes)
}

fn read_comments(path: &Path) -> Result<Vec<Comment>> {
    ensure!(
        !path.is_symlink(),
        aow_i18n::text("inbox.inbox_comments_cannot_be_a_symlink")
    );
    let text = fs::read_to_string(path).with_context(|| {
        aow_i18n::message(
            "inbox.read_comments",
            &[("path", path.display().to_string())],
        )
    })?;
    let mut comments = Vec::new();
    let mut ids = HashSet::new();
    let mut requests = HashSet::new();
    for (index, line) in text.lines().enumerate() {
        let comment: Comment = serde_json::from_str(line).with_context(|| {
            aow_i18n::message(
                "inbox.invalid_comment",
                &[
                    ("path", path.display().to_string()),
                    ("line", (index + 1).to_string()),
                ],
            )
        })?;
        super::store::validate_comment(&comment).map_err(|e| anyhow::anyhow!(e.body.message))?;
        ensure!(
            ids.insert(comment.id.clone()),
            aow_i18n::text("inbox.duplicate_inbox_comment_id")
        );
        if let Some(request) = &comment.request_key {
            ensure!(
                requests.insert(request.clone()),
                aow_i18n::text("inbox.duplicate_inbox_comment_request")
            );
        }
        comments.push(comment);
    }
    Ok(comments)
}
fn read_required<T: DeserializeOwned>(path: &Path) -> Result<T> {
    ensure!(
        !path.is_symlink(),
        aow_i18n::message(
            "inbox.file_symlink",
            &[("path", path.display().to_string())]
        )
    );
    serde_json::from_slice(&fs::read(path)?).with_context(|| {
        aow_i18n::message(
            "inbox.invalid_file",
            &[("path", path.display().to_string())],
        )
    })
}
fn read<T: DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| {
            aow_i18n::message(
                "inbox.invalid_file",
                &[("path", path.display().to_string())],
            )
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    write_bytes(path, &serde_json::to_vec_pretty(value)?)
}
fn write_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context(aow_i18n::text("inbox.missing_parent"))?;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(parent)?;
    let temporary = parent.join(format!(".{}.tmp", aow_id::new_id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod localization_tests {
    use super::*;
    #[test]
    fn comments_errors_localize_without_changing_the_path() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("user-{{path}}.jsonl");
        for (language, prefix) in [
            (aow_i18n::Language::English, "Cannot read Inbox comments"),
            (aow_i18n::Language::Chinese, "无法读取 Inbox 评论"),
        ] {
            let error =
                aow_i18n::with_language(language, || read_comments(&path).unwrap_err().to_string());
            assert!(error.starts_with(prefix), "{error}");
            assert!(error.contains(&path.display().to_string()), "{error}");
        }
        fs::write(&path, "invalid-json").unwrap();
        let error = aow_i18n::with_language(aow_i18n::Language::Chinese, || {
            read_comments(&path).unwrap_err().to_string()
        });
        assert_eq!(error, format!("Inbox 评论无效：{}:1", path.display()));
    }
}
