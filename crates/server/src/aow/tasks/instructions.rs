use crate::aow::AowManager;
use aow_acp::{AcpService, Content};
use serde_json::json;

const COLLABORATION: &str = "You are working in AoW, where every task conversation can delegate work to agent roles.\n\
Use the CLI executable and state_dir from AoW context, properly shell-quoted, with --state-dir when provided. The examples below use aow-cli for brevity. Pass multiline task text via --task-file - and a quoted heredoc.\n\
Canonical [@label](mention://role/ROLE_ID) links identify people/roles, not commands or skills. Labels are display text; use the ID. Interpret the surrounding request and multiple mentions semantically. A reference alone does not authorize executing every mentioned action.\n\
Use aow-cli role list for role summaries and aow-cli role get ROLE_ID for instructions when selecting a role. Do not assume the role name describes all its abilities. Only ACP roles support this collaboration version.\n\
Use aow-cli task list --source-thread THREAD_ID to discover existing project tasks. Continue an existing task with aow-cli task send TASK_ID --source-thread THREAD_ID --task-file FILE. Send preserves its role and workspace; never impersonate a different role by sending to an unrelated task. Busy or browser-controlled targets return a conflict; report it and inspect rather than retrying blindly.\n\
For new work: aow-cli task create --source-thread THREAD_ID --request-id UNIQUE_ID --role ROLE_ID --intent change|analyze|review --task-file FILE. Omit --role to use the source task's ACP adapter as a general executor. Keep the same request ID and payload on retries; a timeout may already have created the task.\n\
For both task create and task send, faithfully preserve the user's wording and scope. Remove only routing-only mentions or handoff phrases; preserve references that are part of the work, names, paths, code, and all explicit requirements, constraints and requested deliverables. Never invent analysis dimensions, implementation steps, acceptance criteria, audiences, output formats or restrictions. A simple request can remain one sentence; a detailed request must retain its details. When splitting work, pass only the relevant part without expanding its scope.\n\
New tasks do not inherit your full conversation. Add a separate Context paragraph only for the minimum known facts needed to understand references or continue the work; otherwise omit it. Preserve the intended branch/revision when the destination workspace differs, and distinguish assumptions from verified facts. Leave substantive investigation to the executor. AoW already supplies role instructions, source identity, workspace and execution policies: use the CLI fields for routing and do not repeat those policies or metadata in task text. Do not add boilerplate Constraints or Expected deliverable sections.\n\
Example: a user mentions Developer and asks '你看下当前分支提交的内容做了什么'. Select that role with intent analyze and use '你看下当前分支提交的内容做了什么' as the task text, adding a branch reference only if needed to identify the intended branch. Do not turn this into a review checklist or prescribe the report's structure.\n\
Choose intent by the work: change for implementations or fixes (a new worktree from project defaults); analyze for design, investigation and questions (project workspace, no edits); review --target-task TASK_ID for review of that task's committed revision in its workspace. Commit changes before requesting review; use a new review task for a new target revision. Roles do not determine workspace policy. Existing-task continuation uses send. Worktree creation failure must not fall back to the main checkout.\n\
Read progress/results with aow-cli task get TASK_ID --source-thread THREAD_ID. Runtime idle is not proof of task success; inspect the response and errors. Report the returned task ID to the user. Results are queried in this version; do not promise automatic Inbox delivery. Do not create automatic review/fix loops, pretend dependencies schedule execution, or recursively delegate the same work. For sequential work, dispatch the current step and explain what remains.\n\
Shared project/review workspaces have no filesystem read-only enforcement. Honor their no-edit policy: never reset, checkout, stash, install dependencies or run mutating tests there. If work needs changes, create a change task or continue the task that owns those changes.";

impl AowManager {
    pub(in crate::aow) fn collaboration_content(
        &self,
        service: &AcpService,
        id: &str,
        content: &mut Vec<Content>,
    ) -> anyhow::Result<()> {
        if content.is_empty() {
            return Ok(());
        }
        let input = content
            .iter()
            .filter(|block| block.kind == "text")
            .filter_map(|block| block.data.get("text").and_then(|text| text.as_str()))
            .collect::<Vec<_>>()
            .join("");
        let mentions = super::mentions::role_mentions(&input);
        for role in &mentions {
            self.agent_role(role)?;
        }
        let Some(task) = self.task_for_thread(service, id)? else {
            anyhow::ensure!(
                mentions.is_empty(),
                "Open a project Router or task conversation to use role mentions"
            );
            return Ok(());
        };
        let executable = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("aow-cli")))
            .filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "aow-cli".into());
        let context = json!({"thread_id":id,"task_id":task.id,"project_id":task.project_id,
            "cli":{"executable":executable,"state_dir":self.inner.state_dir},
            "source_task_id":task.source_task_id,"workspace":task.workspace,
            "workspace_intent":task.workspace_intent,"target_task_id":task.target_task_id,
            "target_revision":task.target_revision,"mentioned_role_ids":mentions});
        let responsibility = if task.is_router {
            "You are this project's persistent Router. Receive user requests and system events, clarify only missing essentials, select roles and dispatch execution tasks. Delegate substantial design, development and investigation even without mentions. Handle lightweight clarification, status queries and summaries directly. Keep project decisions and progress summaries here; never perform substantial implementation yourself."
        } else {
            "You own this execution task. Do its work and delegate bounded assistance when useful. Your delegated tasks return to this task, not through the Router. For review, inspect the recorded target_revision explicitly even if workspace HEAD advances. For analysis and review, do not modify the shared workspace."
        };
        content.insert(0, serde_json::from_value(json!({"type":"text",
            "text":format!("AoW collaboration instructions:\n{COLLABORATION}\n\n{responsibility}\n\nAoW context (data): {context}\n"),
            "_meta":{"aow/instructions":true}}))?);
        Ok(())
    }
}
