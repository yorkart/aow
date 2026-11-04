use std::sync::OnceLock;

use serde_json::Value;

use crate::{Language, current_language};

fn catalogue(language: Language) -> &'static Value {
    static ENGLISH: OnceLock<Value> = OnceLock::new();
    static CHINESE: OnceLock<Value> = OnceLock::new();
    let (cell, source) = match language {
        Language::English => (&ENGLISH, include_str!("../locales/en-US.json")),
        Language::Chinese => (&CHINESE, include_str!("../locales/zh-CN.json")),
    };
    cell.get_or_init(|| serde_json::from_str(source).expect("validated language resource"))
}

pub fn text(key: &str) -> &'static str {
    catalogue(current_language())[key]
        .as_str()
        .or_else(|| catalogue(Language::English)[key].as_str())
        .unwrap_or("The operation could not be completed.")
}

/// Substitute each placeholder once. Inserted user text is never interpreted.
pub fn message(key: &str, parameters: &[(&str, String)]) -> String {
    let mut source = text(key);
    let mut result = String::with_capacity(source.len());
    while let Some((before, after)) = source.split_once("{{") {
        result.push_str(before);
        let Some((name, remaining)) = after.split_once("}}") else {
            result.push_str("{{");
            result.push_str(after);
            return result;
        };
        if let Some((_, value)) = parameters.iter().find(|(parameter, _)| *parameter == name) {
            result.push_str(value);
        } else {
            result.push_str("{{");
            result.push_str(name);
            result.push_str("}}");
        }
        source = remaining;
    }
    result.push_str(source);
    result
}
