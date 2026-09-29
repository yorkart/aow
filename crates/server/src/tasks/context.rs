use aow_protocol::{BoardTask, TaskStatus};

/// Keep application context separate from the user's original Markdown.
pub(super) fn prompt(task: &BoardTask, statuses: &[TaskStatus]) -> String {
    let statuses = statuses
        .iter()
        .map(|status| {
            format!(
                "    <status id=\"{}\">{}</status>",
                xml(&status.id),
                xml(&status.name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<aow_task_context>
  <task_id>{id}</task_id>
  <available_statuses>
{statuses}
  </available_statuses>
  <status_reporting>
    Read status definitions: aow-cli task statuses
    Read current state: aow-cli task get {id}
    Report status: aow-cli task set-status {id} --status STATUS-ID --expected-revision REVISION --reason 'explanation'
    Read the current revision and status definitions before reporting. Choose a status based on actual progress and the user's instructions; do not assume a fixed transition order.
    Statuses describe progress only; do not infer approval or execute additional work from a status label. Follow the user's instructions and continue this conversation when steered.
  </status_reporting>
</aow_task_context>

{title}

{description}",
        id = xml(&task.id),
        title = task.title,
        description = task.description,
    )
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
