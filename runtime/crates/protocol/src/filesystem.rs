use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub readonly: bool,
    pub hidden: bool,
    pub mode: u32,
    pub links: u64,
    pub uid: u32,
    pub gid: u32,
    pub is_symlink: bool,
    pub link_target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryListing {
    pub path: String,
    pub entries: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextFile {
    pub path: String,
    pub content: String,
    pub size: u64,
    pub version: String,
    pub language: String,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteResult {
    pub path: String,
    pub size: u64,
    pub created: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameResult {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}
