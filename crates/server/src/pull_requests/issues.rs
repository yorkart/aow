use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Result, decode, invalid_json, web_link};

#[derive(Deserialize, Serialize)]
struct Issue {
    number: u64,
    title: String,
    status: String,
    url: Option<String>,
    labels: Vec<String>,
    assignees: Vec<String>,
    updated_at: String,
}

#[derive(Deserialize, Serialize)]
struct Label {
    name: String,
    color: String,
    description: String,
}

#[derive(Deserialize, Serialize)]
struct Issues {
    issues: Vec<Issue>,
}

#[derive(Deserialize, Serialize)]
struct Labels {
    labels: Vec<Label>,
}

pub(super) fn validate(operation: &str, result: Value) -> Result<Value> {
    if operation == "issues" {
        let data: Issues = decode(result)?;
        let mut numbers = std::collections::HashSet::new();
        for issue in &data.issues {
            if issue.number == 0
                || !numbers.insert(issue.number)
                || issue.title.trim().is_empty()
                || !matches!(issue.status.as_str(), "open" | "closed")
                || chrono::DateTime::parse_from_rfc3339(&issue.updated_at).is_err()
            {
                return Err(invalid_json("invalid or duplicate Issue"));
            }
            web_link(&serde_json::to_value(issue).unwrap(), "url")?;
        }
        Ok(serde_json::to_value(data).unwrap())
    } else {
        let data: Labels = decode(result)?;
        let mut names = std::collections::HashSet::new();
        for label in &data.labels {
            if label.name.trim().is_empty()
                || !names.insert(&label.name)
                || (!label.color.is_empty()
                    && (label.color.len() != 6
                        || !label.color.bytes().all(|b| b.is_ascii_hexdigit())))
            {
                return Err(invalid_json("invalid or duplicate Issue label"));
            }
        }
        Ok(serde_json::to_value(data).unwrap())
    }
}
