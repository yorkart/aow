use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::aow) enum WorktreeColor {
    #[default]
    Default,
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Gray,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::aow) enum WorktreeIcon {
    #[default]
    Default,
    Cat,
    Dog,
    Rabbit,
    Bird,
    Fish,
    Turtle,
    Squirrel,
    Snail,
    Bug,
    Rat,
    Apple,
    Banana,
    Cherry,
    Citrus,
    Grape,
    Pear,
    Peach,
    Strawberry,
    Watermelon,
    Pineapple,
    Leaf,
    Sprout,
    Flower,
    #[serde(rename = "flower_2")]
    Flower2,
    Clover,
    Wheat,
    TreePine,
    TreeDeciduous,
    TreePalm,
    Trees,
}

#[derive(Debug, Clone, Serialize)]
pub(in crate::aow) struct Worktree {
    pub(in crate::aow) id: String,
    pub(in crate::aow) project_id: String,
    pub(in crate::aow) path: String,
    pub(in crate::aow) branch: String,
    pub(in crate::aow) head: String,
    pub(in crate::aow) is_main: bool,
    pub(in crate::aow) detached: bool,
    pub(in crate::aow) locked: bool,
    pub(in crate::aow) prunable: bool,
    pub(in crate::aow) color: WorktreeColor,
    pub(in crate::aow) icon: WorktreeIcon,
}

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct CreateWorktreeRequest {
    pub(in crate::aow) branch: String,
    pub(in crate::aow) base_ref: String,
    pub(in crate::aow) path: String,
    #[serde(default)]
    pub(in crate::aow) pull_first: bool,
}

#[derive(Debug, Serialize)]
pub(in crate::aow) struct CreateWorktreeResponse {
    pub(in crate::aow) project: Project,
    pub(in crate::aow) worktree: Worktree,
}

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct SetWorktreeColorRequest {
    pub(in crate::aow) path: String,
    pub(in crate::aow) color: WorktreeColor,
}

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct SetWorktreeIconRequest {
    pub(in crate::aow) path: String,
    pub(in crate::aow) icon: WorktreeIcon,
}

#[derive(Debug, Deserialize)]
pub(in crate::aow) struct WorktreeRemovalQuery {
    pub(in crate::aow) path: String,
    #[serde(default)]
    pub(in crate::aow) force: bool,
}

#[derive(Debug)]
pub(in crate::aow) struct WorktreeRemovalInspection {
    pub(in crate::aow) worktree: Worktree,
    pub(in crate::aow) changes: Vec<String>,
    pub(in crate::aow) change_count: usize,
    pub(in crate::aow) truncated: bool,
}

#[derive(Debug, Serialize)]
pub(in crate::aow) struct WorktreeRemovalPreview {
    pub(in crate::aow) worktree: Worktree,
    pub(in crate::aow) dirty: bool,
    pub(in crate::aow) changes: Vec<String>,
    pub(in crate::aow) change_count: usize,
    pub(in crate::aow) truncated: bool,
    pub(in crate::aow) terminal_tabs: usize,
    pub(in crate::aow) agent_tabs: usize,
}

pub(in crate::aow) fn parse_worktrees(project_id: &str, output: &str) -> Vec<Worktree> {
    #[derive(Default)]
    struct Pending {
        path: String,
        head: String,
        branch: String,
        detached: bool,
        locked: bool,
        prunable: bool,
    }
    fn finish(project_id: &str, pending: Pending, index: usize) -> Option<Worktree> {
        if pending.path.is_empty() {
            return None;
        }
        Some(Worktree {
            id: pending.path.clone(),
            project_id: project_id.to_owned(),
            path: pending.path,
            branch: pending
                .branch
                .strip_prefix("refs/heads/")
                .unwrap_or(&pending.branch)
                .to_owned(),
            head: pending.head,
            is_main: index == 0,
            detached: pending.detached,
            locked: pending.locked,
            prunable: pending.prunable,
            color: WorktreeColor::Default,
            icon: WorktreeIcon::Default,
        })
    }

    let mut result = Vec::new();
    let mut pending = Pending::default();
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let Some(worktree) = finish(project_id, pending, result.len()) {
                result.push(worktree);
            }
            pending = Pending::default();
        } else if let Some(value) = line.strip_prefix("worktree ") {
            pending.path = value.to_owned();
        } else if let Some(value) = line.strip_prefix("HEAD ") {
            pending.head = value.to_owned();
        } else if let Some(value) = line.strip_prefix("branch ") {
            pending.branch = value.to_owned();
        } else if line == "detached" {
            pending.detached = true;
        } else if line == "locked" || line.starts_with("locked ") {
            pending.locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            pending.prunable = true;
        }
    }
    result.sort_by(|left, right| {
        right
            .is_main
            .cmp(&left.is_main)
            .then_with(|| left.branch.cmp(&right.branch))
            .then_with(|| left.path.cmp(&right.path))
    });
    result
}
