use super::{
    MAX_ACTIVITY_TEXT, MAX_TURN_ACTIVITIES, Map, Value,
    activity::{abbreviated, command_actions},
    model::{SnapshotActivity, SnapshotMessage, SnapshotTurn},
    tool_details::ToolDetails,
};

#[derive(Debug)]
pub(super) struct TurnDraft {
    pub(super) id: String,
    pub(super) user: Option<SnapshotMessage>,
    pub(super) user_rank: u8,
    pub(super) final_message: Option<SnapshotMessage>,
    pub(super) final_rank: u8,
    pub(super) status: &'static str,
    pub(super) activities: Vec<SnapshotActivity>,
    pub(super) next_activity: usize,
    pub(super) activities_truncated: bool,
    pub(super) usage: Option<crate::sessions::usage::TokenUsage>,
}

impl TurnDraft {
    pub(super) fn new(id: String) -> Self {
        Self {
            id,
            user: None,
            user_rank: 0,
            final_message: None,
            final_rank: 0,
            status: "in_progress",
            activities: Vec::new(),
            next_activity: 0,
            activities_truncated: false,
            usage: None,
        }
    }

    pub(super) fn set_user(&mut self, message: SnapshotMessage, rank: u8) {
        if self
            .user
            .as_ref()
            .is_some_and(|value| value.text == message.text)
        {
            if rank > self.user_rank {
                self.user_rank = rank;
            }
            return;
        }
        if self.user.is_none() || rank > self.user_rank {
            self.user = Some(message);
            self.user_rank = rank;
        }
    }

    pub(super) fn set_final(&mut self, message: SnapshotMessage, rank: u8) {
        if self
            .final_message
            .as_ref()
            .is_some_and(|value| value.text == message.text)
        {
            if rank > self.final_rank {
                self.final_rank = rank;
            }
            return;
        }
        if self.final_message.is_none() || rank >= self.final_rank {
            self.final_message = Some(message);
            self.final_rank = rank;
        }
    }

    pub(super) fn add_commentary(&mut self, message: SnapshotMessage) {
        let text = abbreviated(&message.text, MAX_ACTIVITY_TEXT);
        if self
            .activities
            .last()
            .is_some_and(|item| item.kind == "commentary" && item.text == text)
        {
            return;
        }
        self.next_activity += 1;
        self.push_activity(SnapshotActivity {
            id: format!("progress-{}", self.next_activity),
            kind: "commentary",
            text,
            timestamp: message.timestamp,
            status: None,
            actions: Vec::new(),
            details: None,
        });
    }

    fn push_activity(&mut self, activity: SnapshotActivity) {
        if self.activities.len() == MAX_TURN_ACTIVITIES {
            self.activities.remove(0);
            self.activities_truncated = true;
        }
        self.activities.push(activity);
    }

    // Older transcripts and Claude do not always mark commentary explicitly.
    // A provisional answer followed by another message/tool becomes progress.
    pub(super) fn archive_provisional_final(&mut self) {
        if self.final_rank == 1 {
            if let Some(message) = self.final_message.take() {
                self.add_commentary(message);
            }
            self.final_rank = 0;
        }
    }

    pub(super) fn add_tool(
        &mut self,
        call_id: &str,
        name: &str,
        timestamp: Option<String>,
        status: &'static str,
        record: &Map<String, Value>,
    ) {
        let id = format!("tool-{call_id}");
        let actions = command_actions(record);
        let details = ToolDetails::from_call(record);
        if let Some(item) = self.activities.iter_mut().find(|item| item.id == id) {
            item.status = Some(status);
            item.merge_details(details);
            if !actions.is_empty() {
                item.actions = actions;
            }
            return;
        }
        self.archive_provisional_final();
        self.push_activity(SnapshotActivity {
            id,
            kind: "tool",
            text: abbreviated(name, 160),
            timestamp,
            status: Some(status),
            actions,
            details,
        });
        if self.status == "completed" {
            self.status = "in_progress";
        }
    }

    pub(super) fn finish_tool(
        &mut self,
        call_id: &str,
        status: &'static str,
        details: Option<ToolDetails>,
    ) {
        let id = format!("tool-{call_id}");
        if let Some(item) = self.activities.iter_mut().find(|item| item.id == id) {
            item.status = Some(status);
            item.merge_details(details);
        }
    }

    pub(super) fn finish(mut self) -> Option<SnapshotTurn> {
        if self.status != "in_progress" {
            for item in &mut self.activities {
                if item.status == Some("in_progress") {
                    item.status = Some(if self.status == "interrupted" {
                        "interrupted"
                    } else {
                        "unknown"
                    });
                }
            }
        }
        Some(SnapshotTurn {
            id: self.id,
            status: self.status,
            user: self.user?,
            final_message: self.final_message,
            activities: self.activities,
            activities_truncated: self.activities_truncated,
            usage: self.usage,
        })
    }
}

pub(super) fn ensure_draft<'a>(
    current: &'a mut Option<TurnDraft>,
    fallback_id: &mut usize,
) -> &'a mut TurnDraft {
    current.get_or_insert_with(|| {
        *fallback_id += 1;
        TurnDraft::new(format!("turn-{}", *fallback_id))
    })
}

pub(super) fn push_draft(turns: &mut Vec<SnapshotTurn>, draft: Option<TurnDraft>) {
    if let Some(turn) = draft.and_then(TurnDraft::finish) {
        turns.push(turn);
    }
}
