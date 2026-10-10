//! Preserve Zed's dependent-default retry semantics and conflict termination.
use crate::settings::AgentConfigOptionValue;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn next_default(
    options: &Value,
    defaults: &BTreeMap<String, AgentConfigOptionValue>,
    attempted: &mut Vec<(String, Value)>,
) -> Option<(String, Value)> {
    for option in options.as_array()? {
        let id = option["configId"]
            .as_str()
            .or_else(|| option["id"].as_str())?;
        if attempted
            .iter()
            .any(|(previous, state)| previous == id && state == options)
        {
            continue;
        }
        let Some(default) = defaults.get(id) else {
            continue;
        };
        let value = serde_json::to_value(default).ok()?;
        if option["currentValue"] == value {
            continue;
        }
        attempted.push((id.into(), options.clone()));
        let valid = match option["type"].as_str() {
            Some("boolean") => value.is_boolean(),
            Some("select") => option["options"].as_array().is_some_and(|values| {
                values.iter().any(|entry| {
                    entry["value"] == value
                        || entry["options"]
                            .as_array()
                            .is_some_and(|group| group.iter().any(|entry| entry["value"] == value))
                })
            }),
            _ => false,
        };
        if valid {
            return Some((id.into(), value));
        }
    }
    None
}
