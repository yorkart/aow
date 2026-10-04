mod image;
mod storage;
mod upload;

use std::path::PathBuf;

pub(super) use image::clipboard_image_type;

pub(super) struct ClipboardStorage {
    pub(super) directory: PathBuf,
    remove_on_drop: bool,
    initialization_error: Option<String>,
}
