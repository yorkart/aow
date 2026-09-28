mod directory;
mod error;
mod files;
mod limits;
mod metadata;
mod paths;
mod rename;
mod write;

pub use directory::list_directory;
pub use error::FsError;
pub use files::{open_file, read_text_file};
pub use limits::DEFAULT_MAX_TEXT_BYTES;
pub use metadata::version_for_metadata;
pub use paths::{absolute_path, default_state_dir};
pub use rename::{rename_entry, rename_file};
pub use write::{write_atomic_stream, write_unique_stream};

#[cfg(test)]
mod tests;
