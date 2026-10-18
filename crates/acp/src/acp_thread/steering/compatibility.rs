//! Wire compatibility for the optional steering extension. Never infer support
//! from an agent name, or infer successful completion from quiet output.
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Active,
    Idle,
    Failed,
}

pub(crate) fn activity(update: &Value) -> Option<Activity> {
    if update["sessionUpdate"] != "session_info_update" {
        return None;
    }
    // codex-acp's adapter-owned fallback has no PromptResponse. Its status
    // extension reports quiescence, but does not report the turn's stop reason.
    match update["_meta"]["codex"]["threadStatus"]["type"].as_str()? {
        "active" => Some(Activity::Active),
        "idle" => Some(Activity::Idle),
        "systemError" => Some(Activity::Failed),
        _ => None,
    }
}
