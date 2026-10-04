use super::{
    model::*,
    persistence::{self, Persistence},
};
use crate::HttpError;
use axum::http::StatusCode;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

type Result<T> = std::result::Result<T, HttpError>;
struct State {
    document: Document,
    runs: Vec<Run>,
    comments: BTreeMap<String, Vec<Comment>>,
}
struct Inner {
    state: Mutex<State>,
    persistence: Option<Persistence>,
}
#[derive(Clone)]
pub(crate) struct InboxStore(Arc<Inner>);
impl InboxStore {
    pub(crate) fn in_memory() -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State {
                document: Document::default(),
                runs: vec![],
                comments: BTreeMap::new(),
            }),
            persistence: None,
        }))
    }
    pub(crate) fn persistent(state_dir: &Path) -> anyhow::Result<Self> {
        let (persistence, document, runs) = Persistence::open(state_dir)?;
        let comments = document
            .items
            .iter()
            .map(|item| {
                persistence
                    .comments(&item.id)
                    .map(|comments| (item.id.clone(), comments))
            })
            .collect::<anyhow::Result<_>>()?;
        Ok(Self(Arc::new(Inner {
            state: Mutex::new(State {
                document,
                runs,
                comments,
            }),
            persistence: Some(persistence),
        })))
    }
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.0
            .state
            .lock()
            .map_err(|_| HttpError::internal("Inbox lock poisoned"))
    }
    pub(super) fn snapshot(&self) -> Result<Snapshot> {
        let s = self.lock()?;
        // Only the latest launch per requirement is needed by the list.
        let mut seen = HashSet::new();
        Ok(Snapshot {
            revision: s.document.revision,
            labels: s.document.labels.clone(),
            items: s.document.items.clone(),
            comment_counts: s
                .document
                .items
                .iter()
                .map(|item| {
                    (
                        item.id.clone(),
                        s.comments.get(&item.id).map_or(0, Vec::len),
                    )
                })
                .collect(),
            executions: s
                .runs
                .iter()
                .rev()
                .filter(|r| {
                    seen.insert(&r.item_id) && s.document.items.iter().any(|i| i.id == r.item_id)
                })
                .cloned()
                .collect(),
        })
    }
    pub(super) fn item(&self, id: &str) -> Result<Item> {
        self.lock()?
            .document
            .items
            .iter()
            .find(|i| i.id == id)
            .cloned()
            .ok_or_else(missing)
    }
    fn mutate<T>(&self, change: impl FnOnce(&mut Document) -> Result<T>) -> Result<T> {
        let mut state = self.lock()?;
        let mut next = state.document.clone();
        let result = change(&mut next)?;
        next.revision += 1;
        validate_document(&next)?;
        if let Some(persistence) = &self.0.persistence {
            if let Err(error) = persistence.save_document(&next) {
                // ConfigRepository may save successfully before a Git commit fails.
                // Reflect that durable write instead of accepting stale revisions.
                if let Ok(durable) = persistence.document() {
                    state.document = durable;
                }
                return Err(internal(error));
            }
        }
        state.document = next;
        Ok(result)
    }

    pub(super) fn comments(&self, id: &str) -> Result<Vec<Comment>> {
        let state = self.lock()?;
        if !state.document.items.iter().any(|item| item.id == id) {
            return Err(missing());
        }
        Ok(state.comments.get(id).cloned().unwrap_or_default())
    }

    pub(super) fn append_comment(&self, id: &str, input: AddComment) -> Result<Comment> {
        validate_id(&input.request_key)?;
        let mut state = self.lock()?;
        if !state.document.items.iter().any(|item| item.id == id) {
            return Err(missing());
        }
        let existing = state.comments.get(id).and_then(|comments| {
            comments
                .iter()
                .find(|comment| comment.request_key.as_deref() == Some(input.request_key.as_str()))
        });
        let comment = if let Some(existing) = existing {
            if existing.content != input.content || existing.author != input.author {
                return Err(conflict("这条评论已提交，补充内容请追加新评论。"));
            }
            existing.clone()
        } else {
            Comment {
                id: aow_id::new_id(),
                author: input.author,
                content: input.content,
                created_at: chrono::Utc::now().to_rfc3339(),
                request_key: Some(input.request_key),
            }
        };
        validate_comment(&comment)?;
        if let Some(persistence) = &self.0.persistence {
            if let Err(error) = persistence.append_comment(id, &comment) {
                // An append can be durable even if committing it failed.
                if let Ok(comments) = persistence.comments(id) {
                    state.comments.insert(id.to_owned(), comments);
                }
                return Err(internal(error));
            }
        }
        let comments = state.comments.entry(id.to_owned()).or_default();
        if !comments.iter().any(|existing| existing.id == comment.id) {
            comments.push(comment.clone());
        }
        Ok(comment)
    }
    pub(super) fn capture(&self, input: Capture) -> Result<Item> {
        validate_id(&input.request_key)?;
        validate_markdown(&input.markdown)?;
        self.mutate(|doc| {
            let fingerprint = format!("{:x}", md5::compute(input.markdown.as_bytes()));
            if let Some((id, previous)) = doc.captures.get(&input.request_key) {
                if previous != &fingerprint {
                    return Err(conflict("这次录入已提交，请刷新列表确认后再录入。"));
                }
                return doc
                    .items
                    .iter()
                    .find(|item| &item.id == id)
                    .cloned()
                    .ok_or_else(|| conflict("这次录入已被删除。"));
            }
            let now = chrono::Utc::now().to_rfc3339();
            let item = Item {
                id: aow_id::new_id(),
                markdown: input.markdown,
                project_id: None,
                label_ids: vec![],
                revision: 1,
                created_at: now.clone(),
                updated_at: now,
            };
            doc.captures
                .insert(input.request_key, (item.id.clone(), fingerprint));
            doc.items.push(item.clone());
            Ok(item)
        })
    }
    pub(super) fn update(&self, id: &str, input: Update) -> Result<Item> {
        self.mutate(|doc| {
            let item = doc
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(missing)?;
            revision(item.revision, input.expected_revision)?;
            item.markdown = input.markdown;
            item.project_id = input.project_id;
            item.label_ids = input.label_ids;
            item.revision += 1;
            item.updated_at = chrono::Utc::now().to_rfc3339();
            Ok(item.clone())
        })
    }
    pub(super) fn delete(&self, id: &str, expected: u64) -> Result<()> {
        self.mutate(|doc| {
            let index = doc
                .items
                .iter()
                .position(|i| i.id == id)
                .ok_or_else(missing)?;
            revision(doc.items[index].revision, expected)?;
            doc.items.remove(index);
            Ok(())
        })
    }
    pub(super) fn reorder(&self, input: Reorder) -> Result<()> {
        self.mutate(|doc| {
            revision(doc.revision, input.expected_revision)?;
            let from = doc
                .items
                .iter()
                .position(|i| i.id == input.item_id)
                .ok_or_else(missing)?;
            if input.before_id.as_deref() == Some(input.item_id.as_str()) {
                return Ok(());
            }
            let item = doc.items.remove(from);
            let to = match input.before_id {
                Some(id) => doc
                    .items
                    .iter()
                    .position(|i| i.id == id)
                    .ok_or_else(missing)?,
                None => doc.items.len(),
            };
            doc.items.insert(to, item);
            Ok(())
        })
    }
    pub(super) fn labels(&self, input: LabelsUpdate) -> Result<()> {
        self.mutate(|doc| {
            revision(doc.revision, input.expected_revision)?;
            validate_labels(&input.labels)?;
            let ids: HashSet<_> = input.labels.iter().map(|l| l.id.as_str()).collect();
            for item in &mut doc.items {
                let count = item.label_ids.len();
                item.label_ids.retain(|id| ids.contains(id.as_str()));
                if count != item.label_ids.len() {
                    item.revision += 1;
                    item.updated_at = chrono::Utc::now().to_rfc3339();
                }
            }
            doc.labels = input.labels;
            Ok(())
        })
    }
    pub(super) fn existing_run(&self, item_id: &str, input: &Execute) -> Result<Option<Run>> {
        let state = self.lock()?;
        existing_run(&state.runs, item_id, input)
    }
    pub(super) fn begin_run(
        &self,
        item: &Item,
        input: &Execute,
        cwd: String,
        markdown: String,
        execution_fingerprint: String,
    ) -> Result<(Run, bool)> {
        let mut state = self.lock()?;
        if let Some(run) = existing_run(&state.runs, &item.id, input)? {
            return Ok((run, false));
        }
        let current = state
            .document
            .items
            .iter()
            .find(|i| i.id == item.id)
            .ok_or_else(missing)?;
        revision(current.revision, input.expected_revision)?;
        if state
            .runs
            .iter()
            .any(|r| r.item_id == item.id && r.phase == RunPhase::Starting)
        {
            return Err(conflict("该需求正在启动 Agent，请等待执行结果。"));
        }
        let id = aow_id::new_id();
        let markdown = super::context::task(&item.id, &id, &markdown);
        if markdown.len() > 128 * 1024 {
            return Err(invalid(
                "需求、追加 prompt 与 Inbox 上下文拼接后不能超过 128 KiB。",
            ));
        }
        let run = Run {
            id,
            item_id: item.id.clone(),
            request_key: input.request_key.clone(),
            execution_fingerprint,
            item_revision: item.revision,
            agent: input.agent.clone(),
            project_id: item
                .project_id
                .clone()
                .ok_or_else(|| invalid("请先绑定项目。"))?,
            cwd,
            workspace_mode: input.workspace.workspace_mode,
            markdown,
            phase: RunPhase::Starting,
            tab_id: None,
            pane_id: None,
            error: None,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let mut runs = state.runs.clone();
        runs.push(run.clone());
        self.save_runs(&runs)?;
        state.runs = runs;
        Ok((run, true))
    }
    pub(super) fn set_run_cwd(&self, id: &str, cwd: String) -> Result<()> {
        let mut state = self.lock()?;
        let mut runs = state.runs.clone();
        let run = runs
            .iter_mut()
            .find(|run| run.id == id)
            .ok_or_else(missing)?;
        run.cwd = cwd;
        self.save_runs(&runs)?;
        state.runs = runs;
        Ok(())
    }

    pub(crate) fn is_temporary_workspace(&self, path: &str) -> Result<bool> {
        Ok(self.lock()?.runs.iter().any(|run| {
            run.workspace_mode == aow_workspaces::WorkspaceMode::Temporary
                && run.tab_id.is_some()
                && !path.is_empty()
                && run.cwd == path
        }))
    }
    pub(super) fn finish_run(
        &self,
        id: &str,
        info: std::result::Result<aow_protocol::AgentTerminalInfo, String>,
    ) -> Result<()> {
        let mut state = self.lock()?;
        let mut runs = state.runs.clone();
        let run = runs.iter_mut().find(|r| r.id == id).ok_or_else(missing)?;
        match info {
            Ok(info) => {
                run.tab_id = Some(info.tab_id);
                run.pane_id = Some(info.pane_id);
                run.phase = if info.state.task_submitted {
                    RunPhase::Submitted
                } else {
                    RunPhase::Failed
                };
                run.error = info.state.error.or_else(|| {
                    (run.phase == RunPhase::Failed).then(|| "需求未提交，请查看终端。".into())
                });
            }
            Err(error) => {
                run.phase = RunPhase::Failed;
                run.error = Some(error);
            }
        }
        // Even a failed final journal write must not leave a live UI stuck in Starting.
        let saved = self.save_runs(&runs);
        state.runs = runs;
        saved
    }
    fn save_runs(&self, runs: &Vec<Run>) -> Result<()> {
        if let Some(persistence) = &self.0.persistence {
            persistence::write(&persistence.runs, runs).map_err(internal)?;
        }
        Ok(())
    }
}
fn existing_run(runs: &[Run], item: &str, input: &Execute) -> Result<Option<Run>> {
    let fingerprint = input.fingerprint().map_err(internal)?;
    if let Some(run) = runs.iter().find(|r| r.request_key == input.request_key) {
        if run.item_id != item
            || run.agent != input.agent
            || run.item_revision != input.expected_revision
            || (!run.execution_fingerprint.is_empty() && run.execution_fingerprint != fingerprint)
        {
            return Err(conflict("执行请求已使用，请刷新后重试。"));
        }
        return Ok(Some(run.clone()));
    }
    Ok(None)
}
pub(super) fn validate_document(doc: &Document) -> Result<()> {
    validate_labels(&doc.labels)?;
    let labels: HashSet<_> = doc.labels.iter().map(|l| &l.id).collect();
    let mut ids = HashSet::new();
    for item in &doc.items {
        validate_id(&item.id)?;
        validate_markdown(&item.markdown)?;
        if let Some(project) = &item.project_id {
            validate_id(project)?;
        }
        if !ids.insert(&item.id)
            || item.label_ids.iter().collect::<HashSet<_>>().len() != item.label_ids.len()
            || item.label_ids.iter().any(|id| !labels.contains(id))
        {
            return Err(invalid("需求 ID 或标签无效。"));
        }
    }
    Ok(())
}
fn validate_labels(labels: &[Label]) -> Result<()> {
    if labels.len() > 64 {
        return Err(invalid("最多配置 64 个标签。"));
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for label in labels {
        validate_id(&label.id)?;
        if !ids.insert(&label.id)
            || label.name.trim().is_empty()
            || label.name.chars().count() > 40
            || !names.insert(label.name.trim().to_lowercase())
            || label.color.len() != 7
            || !label.color.starts_with('#')
            || !label.color.as_bytes()[1..]
                .iter()
                .all(u8::is_ascii_hexdigit)
        {
            return Err(invalid(
                "标签名称须唯一且不超过 40 字，颜色须为十六进制色值。",
            ));
        }
    }
    Ok(())
}
pub(super) fn validate_id(id: &str) -> Result<()> {
    if !aow_id::is_valid_id(id) {
        return Err(invalid("ID 无效。"));
    }
    Ok(())
}
fn validate_markdown(markdown: &str) -> Result<()> {
    if markdown.trim().is_empty() || markdown.len() > 128 * 1024 || markdown.contains('\0') {
        return Err(invalid("请输入需求，最多支持 128 KiB 文本。"));
    }
    Ok(())
}
pub(super) fn validate_comment(comment: &Comment) -> Result<()> {
    validate_id(&comment.id)?;
    if let Some(key) = &comment.request_key {
        validate_id(key)?;
    }
    if comment.content.trim().is_empty()
        || comment.content.len() > 128 * 1024
        || comment.content.contains('\0')
        || comment.author.name.trim().is_empty()
        || comment.author.name.chars().count() > 80
        || chrono::DateTime::parse_from_rfc3339(&comment.created_at).is_err()
    {
        return Err(invalid(
            "评论须包含正文和作者，正文最多 128 KiB，作者名称最多 80 字。",
        ));
    }
    Ok(())
}
pub(super) fn revision(current: u64, expected: u64) -> Result<()> {
    if current != expected {
        return Err(conflict(
            "内容已在其他页面更新，请检查最新内容后重试；当前输入已保留。",
        ));
    }
    Ok(())
}
pub(super) fn invalid(message: &str) -> HttpError {
    HttpError::new(StatusCode::BAD_REQUEST, "invalid_inbox", message, None)
}
fn conflict(message: &str) -> HttpError {
    HttpError::new(StatusCode::CONFLICT, "inbox_conflict", message, None)
}
fn missing() -> HttpError {
    HttpError::new(
        StatusCode::NOT_FOUND,
        "inbox_not_found",
        "需求不存在或已被删除。",
        None,
    )
}
fn internal(error: impl std::fmt::Display) -> HttpError {
    HttpError::internal(error.to_string())
}
