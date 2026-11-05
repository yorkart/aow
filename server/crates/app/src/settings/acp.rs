use serde::Serialize;
#[derive(Serialize)]
pub(crate) struct AcpSettingsFile {
    pub path: String,
    pub content: String,
    pub revision: String,
}
