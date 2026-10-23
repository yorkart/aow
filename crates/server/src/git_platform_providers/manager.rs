use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use super::{
    CommitLinks, GitPlatformError, GitPlatformProvider, GitPlatformProviderManager,
    GitPlatformQuery, Result, Target, absolute, git, invalid_json, parse_remote, run_adapter,
    web_link,
};

pub(crate) struct Invocation {
    pub result: Value,
    pub target: Target,
}

impl GitPlatformProviderManager {
    pub async fn targets(
        &self,
        repo: &str,
        operations: &[&str],
        paths: &[PathBuf],
    ) -> Result<Vec<Target>> {
        if operations
            .iter()
            .any(|op| !super::schema::OPERATIONS.contains(op))
        {
            return Err(invalid_json("unsupported Git platform read operation"));
        }
        let providers = self.settings()?.providers;
        let mut targets = Self::targets_for(repo, paths, &providers).await?;
        if !operations.is_empty() {
            let mut supported = HashSet::new();
            for provider in &providers {
                if targets.iter().any(|target| target.provider == provider.id) {
                    let description = self.describe_provider(provider, paths).await?;
                    if operations
                        .iter()
                        .all(|op| description.operations.iter().any(|item| item == op))
                    {
                        supported.insert(provider.id.clone());
                    }
                }
            }
            targets.retain(|target| supported.contains(&target.provider));
        }
        Ok(targets)
    }

    pub(super) async fn describe_provider(
        &self,
        provider: &GitPlatformProvider,
        paths: &[PathBuf],
    ) -> Result<super::description::Description> {
        let script_path = self.config.as_ref().map(|config| {
            config
                .directory()
                .join(super::settings::DIRECTORY)
                .join(&provider.id)
        });
        super::description::describe_at(provider, paths, script_path.as_deref()).await
    }

    pub async fn commit_links(
        &self,
        repo: &str,
        remote: &str,
        commit: &str,
        paths: &[PathBuf],
    ) -> Result<Option<CommitLinks>> {
        let settings = self.settings()?;
        let Some(target) = Self::targets_for(repo, paths, &settings.providers)
            .await?
            .into_iter()
            .find(|target| target.remote == remote)
        else {
            return Ok(None);
        };
        let provider = settings
            .providers
            .iter()
            .find(|p| p.id == target.provider)
            .unwrap();
        let description = self
            .run_provider(
                provider,
                json!({"version":super::schema::VERSION,"operation":"describe","repository":null,"params":{}}),
                None,
                paths,
            )
            .await?;
        let operations = description
            .get("operations")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_json("describe requires operations"))?;
        if !operations
            .iter()
            .any(|op| op.as_str() == Some("repository.commit_links"))
        {
            return Ok(None);
        }
        let result = self.run_provider(
            provider,
            json!({"version":super::schema::VERSION,"operation":"repository.commit_links",
                "repository":{"root":repo,"host":target.host,"path":target.repository,"remote":target.remote},
                "params":{"commit":commit}}),
            Some(Path::new(repo)),
            paths,
        ).await?;
        Ok(Some(CommitLinks {
            remote_url: web_link(&result, "remote_url")?,
            commit_url: web_link(&result, "commit_url")?,
        }))
    }

    pub(super) async fn run_provider(
        &self,
        provider: &GitPlatformProvider,
        request: Value,
        cwd: Option<&Path>,
        paths: &[PathBuf],
    ) -> Result<Value> {
        let script_path = self.config.as_ref().map(|c| {
            c.directory()
                .join(super::settings::DIRECTORY)
                .join(&provider.id)
        });
        run_adapter(provider, request, cwd, paths, script_path.as_deref()).await
    }

    pub(super) async fn targets_for(
        repo: &str,
        paths: &[PathBuf],
        providers: &[GitPlatformProvider],
    ) -> Result<Vec<Target>> {
        absolute(repo)?;
        let names = git(repo, &["remote"], paths).await?;
        let mut targets: Vec<Target> = Vec::new();
        for remote in names.lines().filter(|s| !s.is_empty()) {
            let url = git(repo, &["remote", "get-url", "--", remote], paths).await?;
            let Some((host, repository)) = parse_remote(url.trim()) else {
                continue;
            };
            let Some(provider) = providers.iter().find(|p| p.hosts.contains(&host)) else {
                continue;
            };
            targets.push(Target {
                remote: remote.into(),
                host,
                repository,
                provider: provider.id.clone(),
                provider_name: provider.name.clone(),
            });
        }
        Ok(targets)
    }
    pub async fn call(
        &self,
        query: &GitPlatformQuery,
        operation: &str,
        params: Value,
        paths: &[PathBuf],
    ) -> Result<Invocation> {
        if !super::schema::OPERATIONS.contains(&operation) {
            return Err(invalid_json("unsupported Git platform read operation"));
        }
        let settings = self.settings()?;
        let targets = Self::targets_for(&query.repo, paths, &settings.providers).await?;
        let mut matches: Vec<_> = targets
            .into_iter()
            .filter(|t| {
                query
                    .provider
                    .as_deref()
                    .filter(|p| *p != "auto")
                    .is_none_or(|p| t.provider == p)
                    && query.remote.as_ref().is_none_or(|r| &t.remote == r)
            })
            .collect();
        // Two remote aliases for the same repository do not create ambiguity.
        let mut seen = HashSet::new();
        matches.retain(|t| seen.insert((t.provider.clone(), t.host.clone(), t.repository.clone())));
        let target = match matches.len() {
            0 => {
                return Err(GitPlatformError::Unavailable(
                    aow_i18n::text(
                        "server.no_matching_git_platform_provider_configure_remote_domains_and_scripts",
                    )
                    .into(),
                ));
            }
            1 => matches.remove(0),
            _ => {
                return Err(GitPlatformError::Invalid(
                    aow_i18n::text(
                        "server.multiple_remotes_match_git_platform_providers_choose_a_remote_first",
                    )
                    .into(),
                ));
            }
        };
        let provider = settings
            .providers
            .into_iter()
            .find(|p| p.id == target.provider)
            .ok_or_else(|| {
                GitPlatformError::Unavailable(
                    aow_i18n::text("server.provider_unavailable_refresh").into(),
                )
            })?;
        let description = self.describe_provider(&provider, paths).await?;
        if !description.operations.iter().any(|op| op == operation) {
            return Err(GitPlatformError::Unavailable(format!(
                "Provider {} does not support {operation}",
                provider.id
            )));
        }
        let request = json!({"version":super::schema::VERSION,"operation":operation,
            "repository":{"root":query.repo,"host":target.host,"path":target.repository,"remote":target.remote},"params":params});
        let result = self
            .run_provider(&provider, request, Some(Path::new(&query.repo)), paths)
            .await?;
        Ok(Invocation { result, target })
    }
}
