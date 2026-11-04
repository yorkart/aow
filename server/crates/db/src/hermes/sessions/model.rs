/// Native session data plus the first visible user message in its compression lineage.
#[derive(Debug, PartialEq)]
pub struct SessionRecord {
    pub id: String,
    pub cwd: String,
    pub title: Option<String>,
    pub started_at: f64,
    pub updated_at: f64,
    pub first_user_message: Option<String>,
}
