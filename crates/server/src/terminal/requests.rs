#[derive(Debug, Deserialize)]
pub(super) struct ListQuery {
    pub(super) workspace_root: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AttachQuery {
    pub(super) epoch: Option<String>,
    pub(super) after: Option<u64>,
    pub(super) control: Option<String>,
    pub(super) capabilities: Option<String>,
    pub(super) observer: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) struct AttachOptions<'a> {
    pub(super) epoch: Option<&'a str>,
    pub(super) after: Option<u64>,
    pub(super) controlled: bool,
    pub(super) vt_snapshot: bool,
    pub(super) observer: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateTerminalRequest {
    pub(super) name: Option<String>,
    pub(super) cwd: Option<String>,
    pub(super) workspace_root: Option<String>,
    pub(super) shell: Option<String>,
    pub(super) agent_id: Option<String>,
    pub(super) resume_session_id: Option<String>,
    pub(super) rows: Option<u16>,
    pub(super) cols: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdateTerminalRequest {
    pub(super) name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReorderTerminalsRequest {
    pub(super) workspace_root: String,
    pub(super) tab_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SplitTerminalRequest {
    pub(super) target_pane_id: String,
    pub(super) axis: TerminalSplitAxis,
    pub(super) ratio: Option<f32>,
    pub(super) cwd: Option<String>,
    pub(super) shell: Option<String>,
    pub(super) rows: Option<u16>,
    pub(super) cols: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdateLayoutRequest {
    pub(super) layout: TerminalLayout,
    pub(super) revision: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(super) struct ClipboardImageResponse {
    pub(super) path: String,
    pub(super) mime: &'static str,
    pub(super) size: u64,
    pub(super) expires_at: String,
}
use serde::{Deserialize, Serialize};

use super::*;
