use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub readonly: bool,
    pub hidden: bool,
    pub mode: u32,
    pub links: u64,
    pub uid: u32,
    pub gid: u32,
    pub is_symlink: bool,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryListing {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextFile {
    pub path: String,
    pub content: String,
    pub size: u64,
    pub version: String,
    pub language: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub size: u64,
    pub created: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameResult {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositorySummary {
    pub path: String,
    pub name: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitIgnoredPaths {
    pub repository: Option<String>,
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitFileStatus {
    pub path: String,
    pub index_status: String,
    pub worktree_status: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitStatus {
    pub repository: String,
    pub branch: String,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<GitFileStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommit {
    pub id: String,
    pub short_id: String,
    pub author: String,
    pub authored_at: String,
    pub subject: String,
    pub parents: Vec<String>,
    pub is_pushed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLog {
    pub repository: String,
    pub upstream: Option<String>,
    pub upstream_commit: Option<String>,
    pub commits: Vec<GitCommit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitFile {
    pub path: String,
    pub status: String,
    pub original_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitFiles {
    pub repository: String,
    pub commit: String,
    pub files: Vec<GitCommitFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
    pub date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitStats {
    pub files_changed: u32,
    pub insertions: u64,
    pub deletions: u64,
    pub binary_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitCommitDetail {
    pub repository: String,
    pub id: String,
    pub short_id: String,
    pub author: GitIdentity,
    pub committer: GitIdentity,
    pub subject: String,
    pub body: String,
    pub parents: Vec<String>,
    pub refs: Vec<String>,
    pub stats: GitCommitStats,
    pub upstream: Option<String>,
    pub remote_name: Option<String>,
    /// Optional web links supplied by the configured repository Provider.
    pub remote_url: Option<String>,
    pub commit_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDiff {
    pub repository: String,
    pub path: Option<String>,
    pub staged: bool,
    pub patch: String,
    pub truncated: bool,
    pub original: Option<String>,
    pub modified: Option<String>,
}

/// Platform-independent review data returned by a configured provider adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyPullRequests {
    pub repository: String,
    pub current_branch: String,
    pub current_user: PullRequestUser,
    pub pull_requests: Vec<PullRequestSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestSummary {
    pub number: u64,
    pub status: String,
    pub draft: bool,
    pub title: String,
    pub source_branch: String,
    pub target_branch: String,
    pub url: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestDetail {
    #[serde(flatten)]
    pub summary: PullRequestSummary,
    pub description: String,
    pub changes_count: u32,
    pub commits_count: u32,
    pub review_status: String,
    pub check_summary_status: String,
    pub mergeable: Option<bool>,
    pub reviewers: Vec<PullRequestUser>,
    pub checks: Vec<PullRequestCheck>,
    pub unresolved_threads: Vec<PullRequestThread>,
    pub files: Vec<PullRequestFile>,
    pub author: Option<PullRequestUser>,
    pub labels: Vec<String>,
    pub merge_checks: Vec<PullRequestMergeCheck>,
    pub threads: Vec<PullRequestThread>,
    pub warnings: Vec<String>,
    pub diverged_commits_count: u64,
    pub milestone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestMergeCheck {
    pub name: String,
    pub passed: Option<bool>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestCheck {
    pub id: String,
    pub name: String,
    pub status: String,
    pub conclusion: String,
    pub details_url: Option<String>,
    pub description: String,
    pub text: String,
    pub required: bool,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestThread {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub status: String,
    pub author: String,
    pub body: String,
    pub updated_at: Option<String>,
    pub comments: Vec<PullRequestComment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestFile {
    pub path: String,
    pub change_type: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestDiff {
    pub repository: String,
    pub number: u64,
    pub path: String,
    pub original_path: Option<String>,
    pub original: Option<String>,
    pub modified: Option<String>,
    #[serde(default)]
    pub patch: Option<String>,
    pub binary: bool,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPaneStatus {
    Running,
    Exited,
    Interrupted,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPaneKind {
    #[default]
    Terminal,
    Agent,
}

impl TerminalPaneKind {
    fn is_terminal(&self) -> bool {
        *self == Self::Terminal
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalSplitAxis {
    /// Places the children left-to-right.
    Row,
    /// Places the children top-to-bottom.
    Column,
}

fn default_terminal_split_ratio() -> f32 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalLayout {
    Pane {
        pane_id: String,
    },
    Split {
        axis: TerminalSplitAxis,
        #[serde(default = "default_terminal_split_ratio")]
        ratio: f32,
        first: Box<TerminalLayout>,
        second: Box<TerminalLayout>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalPane {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub cwd: String,
    pub shell: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "TerminalPaneKind::is_terminal")]
    pub kind: TerminalPaneKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Registration used to launch the agent; distinct from its product type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_profile_id: Option<String>,
    /// Present only for CLI-created, initially hidden interactive agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_terminal: Option<AgentTerminalState>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub restart_on_daemon_restart: bool,
    pub status: TerminalPaneStatus,
    pub rows: u16,
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentTerminalPhase {
    Starting,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentTerminalState {
    pub phase: AgentTerminalPhase,
    pub error: Option<String>,
    pub task_submitted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTerminalCreate {
    pub agent: String,
    pub project_id: String,
    pub cwd: String,
    pub task: Option<String>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTerminalSubmit {
    pub task: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTerminalInfo {
    pub pane_id: String,
    pub tab_id: String,
    pub cwd: String,
    pub agent: String,
    pub status: TerminalPaneStatus,
    #[serde(flatten)]
    pub state: AgentTerminalState,
}

/// Text from the current VT viewport, never an accumulation of raw redraws.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalScreen {
    pub generation: String,
    pub applied_offset: u64,
    pub cols: u16,
    pub rows: u16,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalTab {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_is_custom: Option<bool>,
    pub workspace_root: String,
    pub layout: TerminalLayout,
    pub panes: Vec<TerminalPane>,
    pub revision: u64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TerminalTabList {
    pub tabs: Vec<TerminalTab>,
}

/// Identity and version information returned by a terminald instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminaldHealth {
    pub service: String,
    pub instance_id: String,
    pub version: String,
    /// Optional for compatibility with older daemons; used by launchd activation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

/// The immutable process specification used for idempotent runtime creation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRuntimeSpec {
    pub cwd: String,
    pub shell: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    pub rows: u16,
    pub cols: u16,
}

/// Runtime status uses the same wire values as the browser terminal protocol.
pub type TerminalRuntimeStatus = TerminalPaneStatus;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRuntime {
    pub id: String,
    pub cwd: String,
    pub shell: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
    /// Launch-only values are retained inside terminald for idempotency but
    /// are never serialized back to HTTP clients.
    #[serde(skip)]
    pub environment: BTreeMap<String, String>,
    pub status: TerminalRuntimeStatus,
    pub rows: u16,
    pub cols: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
}

impl TerminalRuntime {
    /// Returns the runtime's currently reported process fields and PTY size.
    ///
    /// The daemon compares idempotent PUT requests against its immutable
    /// creation spec internally; after a WebSocket resize, this value reflects
    /// the live size and is therefore not necessarily that creation spec.
    pub fn spec(&self) -> TerminalRuntimeSpec {
        TerminalRuntimeSpec {
            cwd: self.cwd.clone(),
            shell: self.shell.clone(),
            arguments: self.arguments.clone(),
            environment: self.environment.clone(),
            rows: self.rows,
            cols: self.cols,
        }
    }
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalRuntimeList {
    pub runtimes: Vec<TerminalRuntime>,
}

/// Live agent identities by runtime ID; null means no agent was detected.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalAgentList {
    pub agents: BTreeMap<String, Option<String>>,
    /// Last nonempty OSC 0/2 title for each running runtime, independent of attachments.
    /// Missing entries mean no title is available; older daemons omit this field.
    #[serde(default)]
    pub titles: BTreeMap<String, String>,
    /// Foreground agent processes only. Missing on older daemons and platforms
    /// without process inspection; title-based selection remains available.
    #[serde(default)]
    pub processes: BTreeMap<String, TerminalAgentProcess>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalAgentProcess {
    pub pid: i32,
    /// Opaque OS process start identity distinguishes PID reuse. Linux uses
    /// clock ticks; macOS uses seconds and microseconds. Compare as a string.
    pub start_time: String,
    pub cwd: String,
}

/// Text messages accepted by the runtime attach WebSocket. Binary messages are
/// terminal input bytes and therefore are intentionally not represented here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalAttachClientMessage {
    Observe,
    Claim {
        force: bool,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Acknowledged PTY input for programmatic clients.
    Write {
        request_id: String,
        data: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TerminalControlState {
    Claimed,
    /// The attachment receives terminal output but cannot write input or
    /// change the PTY geometry while another attachment controls it.
    Observing,
    Waiting,
}

/// Text messages produced by the runtime attach WebSocket. Terminal output and
/// scrollback are sent as binary WebSocket messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalAttachServerMessage {
    Written {
        request_id: String,
    },
    Control {
        state: TerminalControlState,
    },
    Stream {
        epoch: String,
        offset: u64,
        reset: bool,
        replay_bytes: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore: Option<String>,
        /// Original width of a serialized VT snapshot carried in `restore`.
        ///
        /// These dimensions are absent for the legacy mode-only repair
        /// sequence. When present, both dimensions must be present and the
        /// receiver must resize its emulator before applying `restore`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_cols: Option<u16>,
        /// Original height of a serialized VT snapshot carried in `restore`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        restore_rows: Option<u16>,
    },
    Resized {
        cols: u16,
        rows: u16,
    },
    Status {
        status: TerminalRuntimeStatus,
        exit_code: Option<u32>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_kind_uses_snake_case() {
        assert_eq!(
            serde_json::to_string(&FileKind::Directory).unwrap(),
            "\"directory\""
        );
    }

    #[test]
    fn terminal_layout_uses_the_shared_axis_schema() {
        let layout = TerminalLayout::Split {
            axis: TerminalSplitAxis::Row,
            ratio: 0.4,
            first: Box::new(TerminalLayout::Pane {
                pane_id: "left".to_owned(),
            }),
            second: Box::new(TerminalLayout::Pane {
                pane_id: "right".to_owned(),
            }),
        };
        let value = serde_json::to_value(layout).unwrap();
        assert_eq!(value["type"], "split");
        assert_eq!(value["axis"], "row");
        assert_eq!(value["first"]["pane_id"], "left");
        assert!(value.get("direction").is_none());
    }

    #[test]
    fn terminal_pane_name_defaults_for_legacy_state() {
        let pane: TerminalPane = serde_json::from_value(serde_json::json!({
            "id": "pane-1",
            "cwd": "/workspace/project",
            "shell": "/bin/bash",
            "status": "running",
            "rows": 24,
            "cols": 80,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        }))
        .unwrap();

        assert_eq!(pane.name, "");
        assert_eq!(pane.kind, TerminalPaneKind::Terminal);
        assert!(pane.arguments.is_empty());
        assert!(pane.agent_id.is_none());
        assert!(pane.restart_on_daemon_restart);
        let value = serde_json::to_value(pane).unwrap();
        assert!(value.get("arguments").is_none());
        assert!(value.get("kind").is_none());
        assert!(value.get("agent_id").is_none());
        assert!(value.get("restart_on_daemon_restart").is_none());
    }

    #[test]
    fn terminal_attach_messages_match_the_browser_wire_format() {
        assert_eq!(
            serde_json::to_value(TerminalAttachClientMessage::Claim { force: true }).unwrap(),
            serde_json::json!({"type": "claim", "force": true})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachClientMessage::Claim { force: false }).unwrap(),
            serde_json::json!({"type": "claim", "force": false})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachClientMessage::Resize {
                cols: 120,
                rows: 40
            })
            .unwrap(),
            serde_json::json!({"type": "resize", "cols": 120, "rows": 40})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachServerMessage::Control {
                state: TerminalControlState::Waiting,
            })
            .unwrap(),
            serde_json::json!({"type": "control", "state": "waiting"})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachServerMessage::Control {
                state: TerminalControlState::Claimed,
            })
            .unwrap(),
            serde_json::json!({"type": "control", "state": "claimed"})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachServerMessage::Control {
                state: TerminalControlState::Observing,
            })
            .unwrap(),
            serde_json::json!({"type": "control", "state": "observing"})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachServerMessage::Status {
                status: TerminalPaneStatus::Exited,
                exit_code: Some(7),
            })
            .unwrap(),
            serde_json::json!({"type": "status", "status": "exited", "exit_code": 7})
        );
        assert_eq!(
            serde_json::to_value(TerminalAttachServerMessage::Stream {
                epoch: "runtime-epoch".to_owned(),
                offset: 42,
                reset: true,
                replay_bytes: 1024,
                restore: Some("\u{1b}[?2004h".to_owned()),
                restore_cols: Some(120),
                restore_rows: Some(40),
            })
            .unwrap(),
            serde_json::json!({
                "type": "stream",
                "epoch": "runtime-epoch",
                "offset": 42,
                "reset": true,
                "replay_bytes": 1024,
                "restore": "\u{1b}[?2004h",
                "restore_cols": 120,
                "restore_rows": 40
            })
        );
        let legacy_stream: TerminalAttachServerMessage =
            serde_json::from_value(serde_json::json!({
                "type": "stream",
                "epoch": "runtime-epoch",
                "offset": 42,
                "reset": false,
                "replay_bytes": 0
            }))
            .unwrap();
        assert_eq!(
            legacy_stream,
            TerminalAttachServerMessage::Stream {
                epoch: "runtime-epoch".to_owned(),
                offset: 42,
                reset: false,
                replay_bytes: 0,
                restore: None,
                restore_cols: None,
                restore_rows: None,
            }
        );
        let legacy_stream_json = serde_json::to_value(legacy_stream).unwrap();
        assert!(legacy_stream_json.get("restore").is_none());
        assert!(legacy_stream_json.get("restore_cols").is_none());
        assert!(legacy_stream_json.get("restore_rows").is_none());
        assert_eq!(
            serde_json::to_value(TerminalAttachServerMessage::Resized {
                cols: 120,
                rows: 40,
            })
            .unwrap(),
            serde_json::json!({"type": "resized", "cols": 120, "rows": 40})
        );
    }

    #[test]
    fn terminal_stream_restore_metadata_remains_backward_compatible() {
        let legacy_mode_restore: TerminalAttachServerMessage =
            serde_json::from_value(serde_json::json!({
                "type": "stream",
                "epoch": "runtime-epoch",
                "offset": 42,
                "reset": true,
                "replay_bytes": 0,
                "restore": "\u{001b}[?2004h"
            }))
            .unwrap();
        assert_eq!(
            legacy_mode_restore,
            TerminalAttachServerMessage::Stream {
                epoch: "runtime-epoch".to_owned(),
                offset: 42,
                reset: true,
                replay_bytes: 0,
                restore: Some("\u{1b}[?2004h".to_owned()),
                restore_cols: None,
                restore_rows: None,
            }
        );

        for dimension in [1_u16, 1000_u16] {
            let snapshot: TerminalAttachServerMessage = serde_json::from_value(serde_json::json!({
                "type": "stream",
                "epoch": "runtime-epoch",
                "offset": 42,
                "reset": true,
                "replay_bytes": 0,
                "restore": "snapshot",
                "restore_cols": dimension,
                "restore_rows": dimension
            }))
            .unwrap();
            assert_eq!(
                snapshot,
                TerminalAttachServerMessage::Stream {
                    epoch: "runtime-epoch".to_owned(),
                    offset: 42,
                    reset: true,
                    replay_bytes: 0,
                    restore: Some("snapshot".to_owned()),
                    restore_cols: Some(dimension),
                    restore_rows: Some(dimension),
                }
            );
        }
    }
}
