use super::{Map, Serialize, Value, content_text};

// Keep both ends: command failures often report their cause after a long log.
const MAX_TOOL_TEXT: usize = 16_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(super) struct ToolText {
    pub text: String,
    pub truncated: bool,
}

impl ToolText {
    fn new(text: String) -> Option<Self> {
        if text.trim().is_empty() {
            return None;
        }
        let truncated = text.chars().count() > MAX_TOOL_TEXT;
        let text = if truncated {
            let head: String = text.chars().take(MAX_TOOL_TEXT / 2).collect();
            let tail: String = text
                .chars()
                .rev()
                .take(MAX_TOOL_TEXT / 2)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("{head}\n… 中间内容已截断 …\n{tail}")
        } else {
            text
        };
        Some(Self { text, truncated })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(super) struct ToolDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<ToolText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ToolText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<ToolText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<ToolText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ToolText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl ToolDetails {
    pub fn from_call(record: &Map<String, Value>) -> Option<Self> {
        let input = record.get("arguments").or_else(|| record.get("input"));
        let decoded = input
            .and_then(Value::as_str)
            .and_then(|text| serde_json::from_str::<Value>(text).ok());
        let input = decoded.as_ref().or(input);
        let parameters = input.and_then(Value::as_object);
        let field = |names: &[&str]| {
            names
                .iter()
                .find_map(|name| record.get(*name).filter(|value| !value.is_null()))
                .or_else(|| {
                    parameters.and_then(|args| names.iter().find_map(|name| args.get(*name)))
                })
        };
        let mut details = Self::from_result(record).unwrap_or_default();
        details.command = field(&["command", "cmd"])
            .and_then(command_text)
            .and_then(ToolText::new);
        details.cwd = field(&["cwd", "workdir"])
            .and_then(Value::as_str)
            .and_then(|text| ToolText::new(text.to_owned()));
        details.input = input.and_then(value_text).and_then(ToolText::new);
        details.nonempty()
    }

    pub fn from_result(record: &Map<String, Value>) -> Option<Self> {
        let mut details = record
            .get("output")
            .and_then(|value| Self::from_output(Some(value)))
            .unwrap_or_default();
        if let Some(output) = [
            "stdout",
            "aggregated_output",
            "formatted_output",
            "content",
            "result",
        ]
        .iter()
        .find_map(|name| {
            record
                .get(*name)
                .and_then(result_text)
                .and_then(ToolText::new)
        }) {
            details.output = Some(output);
        }
        details.error = ["stderr", "error"]
            .iter()
            .find_map(|name| {
                record
                    .get(*name)
                    .and_then(result_text)
                    .and_then(ToolText::new)
            })
            .or(details.error);
        if details.output.is_some() && details.output == details.error {
            details.output = None;
        }
        if details.error.is_none()
            && record.get("is_error").or_else(|| record.get("isError")) == Some(&Value::Bool(true))
        {
            details.error = details.output.take();
        }
        details.exit_code = record
            .get("exit_code")
            .and_then(Value::as_i64)
            .or(details.exit_code);
        details.duration_ms = record
            .get("duration_ms")
            .and_then(Value::as_u64)
            .or_else(|| {
                record.get("duration").and_then(|duration| {
                    Some(
                        duration
                            .get("secs")?
                            .as_u64()?
                            .saturating_mul(1000)
                            .saturating_add(
                                duration.get("nanos").and_then(Value::as_u64).unwrap_or(0)
                                    / 1_000_000,
                            ),
                    )
                })
            })
            .or_else(|| {
                record
                    .get("wall_time_seconds")
                    .and_then(Value::as_f64)
                    .filter(|seconds| *seconds >= 0.0)
                    .map(|seconds| (seconds * 1000.0) as u64)
            })
            .or(details.duration_ms);
        details.nonempty()
    }

    pub fn from_output(output: Option<&Value>) -> Option<Self> {
        let output = output?;
        if let Some(text) = output.as_str()
            && let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(text)
        {
            return Self::from_output(Some(&value));
        }
        if let Some(record) = output.as_object() {
            return Self::from_result(record).or_else(|| {
                Some(Self {
                    output: result_text(output).and_then(ToolText::new),
                    ..Self::default()
                })
                .and_then(Self::nonempty)
            });
        }
        let text = result_text(output)?;
        let exit_code = text.lines().find_map(|line| {
            line.strip_prefix("Process exited with code ")?
                .trim()
                .parse()
                .ok()
        });
        Self {
            output: ToolText::new(text),
            exit_code,
            ..Self::default()
        }
        .nonempty()
    }

    pub fn merge(&mut self, newer: Self) {
        // Start/completion projections often omit fields already recorded by the call.
        macro_rules! merge {
            ($($field:ident),+ $(,)?) => { $(if newer.$field.is_some() { self.$field = newer.$field; })+ };
        }
        merge!(command, input, cwd, output, error, exit_code, duration_ms);
    }

    fn nonempty(self) -> Option<Self> {
        (self != Self::default()).then_some(self)
    }
}

fn value_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => Some(text.clone()),
        _ => serde_json::to_string_pretty(value).ok(),
    }
}

fn result_text(value: &Value) -> Option<String> {
    match value {
        Value::Array(items)
            if items.iter().any(|item| {
                matches!(
                    item.get("type").and_then(Value::as_str),
                    Some(
                        "text"
                            | "Text"
                            | "input_text"
                            | "output_text"
                            | "image"
                            | "audio"
                            | "resource"
                            | "resource_link"
                    )
                )
            }) =>
        {
            content_text(value)
        }
        Value::Object(record) if record.contains_key("content") => {
            record.get("content").and_then(result_text)
        }
        Value::Object(record) if record.contains_key("text") => content_text(value),
        _ => value_text(value),
    }
}

fn command_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    let args: Vec<_> = value
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<_>>()?;
    if args.len() == 3
        && matches!(args[1], "-c" | "-lc" | "-ic")
        && matches!(
            args[0].rsplit('/').next(),
            Some("bash" | "sh" | "zsh" | "dash" | "ksh")
        )
    {
        return Some(args[2].to_owned());
    }
    Some(
        args.iter()
            .map(|arg| {
                if !arg.is_empty()
                    && arg
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_./-=:".contains(c))
                {
                    (*arg).to_owned()
                } else {
                    format!("'{}'", arg.replace('\'', "'\\''"))
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}
