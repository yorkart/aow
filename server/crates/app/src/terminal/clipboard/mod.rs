#[cfg(test)]
mod image;
#[cfg(test)]
mod storage;
mod upload;
#[cfg(test)]
mod upload_fixture;

#[cfg(test)]
use std::path::PathBuf;

#[cfg(test)]
pub(super) use image::clipboard_image_type;

#[cfg(test)]
pub(super) struct ClipboardStorage {
    pub(super) directory: PathBuf,
    remove_on_drop: bool,
    initialization_error: Option<String>,
}
