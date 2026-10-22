//! Discover validated providers from a directory, without platform-specific names.
use super::{
    GitPlatformProvider, SCRIPT_LIMIT, description::describe_at, validation::valid_filename,
};
use anyhow::{Context, ensure};
use std::{collections::HashSet, path::Path};

pub(super) fn read_script(path: &Path) -> anyhow::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(metadata.is_file(), "provider must be a regular file");
    ensure!(
        metadata.len() <= SCRIPT_LIMIT as u64,
        "provider exceeds 1 MiB"
    );
    Ok(std::fs::read_to_string(path)?)
}

pub(super) fn load(directory: &Path) -> anyhow::Result<Vec<GitPlatformProvider>> {
    match std::fs::symlink_metadata(directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        result => ensure!(
            result?.is_dir(),
            "provider directory must be a real directory"
        ),
    }
    let mut files = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort();
    // Startup is synchronous, including when called inside an active Tokio runtime.
    std::thread::spawn(move || -> anyhow::Result<Vec<GitPlatformProvider>> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            let paths: Vec<_> = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
            let mut providers = Vec::new();
            let mut names = HashSet::new();
            for file in files {
                if file.extension().is_none_or(|ext| ext != "mjs") {
                    continue;
                }
                let loaded = async {
                    let id = file.file_name().and_then(|name| name.to_str()).context("invalid provider filename")?;
                    ensure!(valid_filename(id), "invalid .mjs provider filename");
                    ensure!(!names.contains(&id.to_ascii_lowercase()), "case-insensitive provider filename collision");
                    ensure!(providers.len() < 32, "at most 32 providers are allowed");
                    let mut provider = GitPlatformProvider {
                        id: id.to_owned(), name: String::new(), hosts: vec![], enabled: true,
                        script: read_script(&file)?,
                    };
                    let metadata = describe_at(&provider, &paths, Some(&file)).await?;
                    provider.name = metadata.name;
                    provider.hosts = metadata.hosts;
                    Ok::<_, anyhow::Error>(provider)
                }.await;
                match loaded {
                    Ok(provider) => {
                        names.insert(provider.id.to_ascii_lowercase());
                        providers.push(provider);
                    }
                    Err(error) => tracing::warn!(path = %file.display(), error = %error, "Skipping invalid Git platform provider"),
                }
            }
            Ok(providers)
        })
    }).join().map_err(|_| anyhow::anyhow!("provider discovery failed"))?
}
