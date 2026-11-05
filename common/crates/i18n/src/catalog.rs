use crate::{Language, current_language};
use serde_json::Value;
use std::sync::OnceLock;
pub struct Catalog {
    english: &'static str,
    chinese: &'static str,
    values: [OnceLock<Value>; 2],
}
impl Catalog {
    pub const fn new(english: &'static str, chinese: &'static str) -> Self {
        Self {
            english,
            chinese,
            values: [OnceLock::new(), OnceLock::new()],
        }
    }
    fn catalogue(&'static self, language: Language) -> &'static Value {
        let (i, source) = match language {
            Language::English => (0, self.english),
            Language::Chinese => (1, self.chinese),
        };
        self.values[i]
            .get_or_init(|| serde_json::from_str(source).expect("validated language resource"))
    }
    pub fn text(&'static self, key: &str) -> &'static str {
        self.catalogue(current_language())[key]
            .as_str()
            .or_else(|| self.catalogue(Language::English)[key].as_str())
            .unwrap_or("The operation could not be completed.")
    }
    /// Substitute each placeholder once. Inserted user text is never interpreted.
    pub fn message(&'static self, key: &str, parameters: &[(&str, String)]) -> String {
        let mut source = self.text(key);
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
}
