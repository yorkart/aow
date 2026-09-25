//! Transport-independent content. Callers assemble business-specific messages.

#[derive(Clone)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub url: Option<String>,
}

#[derive(Clone)]
pub struct Message {
    pub title: String,
    pub fields: Vec<Field>,
    pub body_label: String,
    pub body: String,
    pub markdown: bool,
    pub error: bool,
}

impl Message {
    /// A single bounded notification, reserving space for complete source URLs.
    /// This is our text budget, not a claimed upstream protocol limit.
    /// Metadata uses paragraphs so rich-text clients do not fold its line breaks.
    /// The body keeps its original Markdown layout.
    pub fn text(&self, limit: usize) -> String {
        let title = truncate(&self.title, 160.min(limit));
        let mut metadata = String::new();
        for field in &self.fields {
            let value = match &field.url {
                Some(url) => format!(
                    "\n\n{}：{}\n\n{url}",
                    truncate(&field.label, 40),
                    paragraphs(&truncate(&field.value, 80))
                ),
                None => format!(
                    "\n\n{}：{}",
                    truncate(&field.label, 40),
                    paragraphs(&truncate(&field.value, 160))
                ),
            };
            if title.chars().count() + metadata.chars().count() + value.chars().count() + 300
                <= limit
            {
                metadata.push_str(&value);
            }
        }
        let prefix = format!(
            "{title}{metadata}\n\n{}：\n\n",
            truncate(&self.body_label, 40)
        );
        let budget = limit.saturating_sub(prefix.chars().count());
        truncate(&format!("{prefix}{}", truncate(&self.body, budget)), limit)
    }
}

fn paragraphs(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    if limit == 0 {
        return String::new();
    }
    format!("{}…", text.chars().take(limit - 1).collect::<String>())
}
