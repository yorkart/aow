#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(not(unix))]
use std::time::UNIX_EPOCH;

pub fn version_for_metadata(metadata: &std::fs::Metadata) -> String {
    version_for_metadata_impl(metadata)
}

#[cfg(unix)]
fn version_for_metadata_impl(metadata: &std::fs::Metadata) -> String {
    format!(
        "{:x}-{:x}-{:x}-{:x}-{:x}-{:x}",
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime_nsec(),
    )
}

#[cfg(not(unix))]
fn version_for_metadata_impl(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{:x}-{:x}", metadata.len(), modified)
}
