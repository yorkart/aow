//! The three-node XML context submitted alongside the original requirement.

pub(super) fn task(requirement_id: &str, execution_id: &str, markdown: &str) -> String {
    let description = "This task comes from AoW Inbox. You can use aow-cli inbox commands to retrieve requirements and comments, update requirements, and append comments.";
    format!(
        "<aow-inbox>\n  <requirement id=\"{}\" />\n  <execution id=\"{}\" />\n  <description>{}</description>\n</aow-inbox>\n\n{markdown}",
        xml(requirement_id),
        xml(execution_id),
        xml(description)
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
