use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorktreeColor {
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
pub(crate) enum WorktreeIcon {
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
pub(crate) struct Worktree {
    pub(crate) id: String,
    pub(crate) project_id: String,
    pub(crate) path: String,
    pub(crate) branch: String,
    pub(crate) head: String,
    pub(crate) is_main: bool,
    pub(crate) detached: bool,
    pub(crate) locked: bool,
    pub(crate) prunable: bool,
    pub(crate) color: WorktreeColor,
    pub(crate) icon: WorktreeIcon,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateWorktreeRequest {
    pub(crate) branch: String,
    pub(crate) base_ref: String,
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) pull_first: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct ProjectBranches {
    pub(crate) branches: Vec<String>,
    pub(crate) default_branch: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CreateWorktreeResponse {
    pub(crate) project: Project,
    pub(crate) worktree: Worktree,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetWorktreeColorRequest {
    pub(crate) path: String,
    pub(crate) color: WorktreeColor,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SetWorktreeIconRequest {
    pub(crate) path: String,
    pub(crate) icon: WorktreeIcon,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WorktreeRemovalQuery {
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) force: bool,
}

#[derive(Debug)]
pub(crate) struct WorktreeRemovalInspection {
    pub(crate) worktree: Worktree,
    pub(crate) changes: Vec<String>,
    pub(crate) change_count: usize,
    pub(crate) truncated: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct WorktreeRemovalPreview {
    pub(crate) worktree: Worktree,
    pub(crate) dirty: bool,
    pub(crate) changes: Vec<String>,
    pub(crate) change_count: usize,
    pub(crate) truncated: bool,
    pub(crate) terminal_tabs: usize,
    pub(crate) agent_tabs: usize,
}

pub(crate) fn parse_worktrees(project_id: &str, output: &str) -> Vec<Worktree> {
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
