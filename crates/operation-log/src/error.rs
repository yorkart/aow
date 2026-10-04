use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("日志读写失败：{0}")]
    Io(#[from] io::Error),
    #[error("日志格式错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("日志游标无效")]
    InvalidCursor,
    #[error("日志文件已过期或发生变更，请刷新日志")]
    CursorExpired,
    #[error("日志查询参数无效")]
    InvalidOptions,
    #[error("单条日志超过大小限制")]
    RecordTooLarge,
}
