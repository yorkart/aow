use aow_protocol::{MyPullRequests, PullRequestDetail, PullRequestDiff};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use super::{
    CommitLinks, Provider, ProviderManager, PullRequestError, Result, ReviewQuery, Target,
    absolute, decode, git, invalid_json, parse_remote, positive, run_adapter, safe_path, web_link,
};

impl ProviderManager {
    pub async fn targets(&self, repo: &str, paths: &[PathBuf]) -> Result<Vec<Target>> {
        let providers = self.settings()?.providers;
        Self::targets_for(repo, paths, &providers).await
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
                json!({"version":2,"operation":"describe","repository":null,"params":{}}),
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
            .any(|op| op.as_str() == Some("commit_links"))
        {
            return Ok(None);
        }
        let result = self.run_provider(
            provider,
            json!({"version":2,"operation":"commit_links",
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
        provider: &Provider,
        request: Value,
        cwd: Option<&Path>,
        paths: &[PathBuf],
    ) -> Result<Value> {
        let script_path = self.config.as_ref().map(|c| {
            c.directory().join(format!(
                "review-providers/{:x}.py",
                md5::compute(provider.script.as_bytes())
            ))
        });
        run_adapter(provider, request, cwd, paths, script_path.as_deref()).await
    }

    pub(super) async fn targets_for(
        repo: &str,
        paths: &[PathBuf],
        providers: &[Provider],
    ) -> Result<Vec<Target>> {
        absolute(repo)?;
        let names = git(repo, &["remote"], paths).await?;
        let mut targets: Vec<Target> = Vec::new();
        for remote in names.lines().filter(|s| !s.is_empty()) {
            let url = git(repo, &["remote", "get-url", "--", remote], paths).await?;
            let Some((host, repository)) = parse_remote(url.trim()) else {
                continue;
            };
            let Some(provider) = providers
                .iter()
                .find(|p| p.enabled && p.hosts.contains(&host))
            else {
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
        query: &ReviewQuery,
        operation: &str,
        params: Value,
        paths: &[PathBuf],
    ) -> Result<Value> {
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
            0 => return Err(PullRequestError::Unavailable(
                "未找到匹配的 PR Provider，请在 Settings → Pull Requests 配置 remote 域名和脚本。"
                    .into(),
            )),
            1 => matches.remove(0),
            _ => {
                return Err(PullRequestError::Invalid(
                    "多个 remote 匹配 PR Provider，请先选择 remote。".into(),
                ));
            }
        };
        let provider = settings
            .providers
            .into_iter()
            .find(|p| p.id == target.provider && p.enabled)
            .ok_or_else(|| PullRequestError::Unavailable("Provider 已停用，请刷新。".into()))?;
        if matches!(operation, "issues" | "issue_labels") {
            let description = self
                .run_provider(
                    &provider,
                    json!({"version":2,"operation":"describe","repository":null,"params":{}}),
                    None,
                    paths,
                )
                .await?;
            let operations = description
                .get("operations")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid_json("describe requires operations"))?;
            if !operations.iter().any(|op| op.as_str() == Some(operation)) {
                return Err(PullRequestError::Unavailable(format!(
                    "{} 尚未支持 Issue 来源（缺少 {operation}），请在 Settings → Pull Requests 更新 Provider 脚本。",
                    provider.name
                )));
            }
        }
        let request = json!({"version":2,"operation":operation,
            "repository":{"root":query.repo,"host":target.host,"path":target.repository,"remote":target.remote},"params":params});
        let result = self
            .run_provider(&provider, request, Some(Path::new(&query.repo)), paths)
            .await?;
        let mut result = match operation {
            "issues" | "issue_labels" => super::issues::validate(operation, result)?,
            "list" => {
                let mut data: MyPullRequests = decode(result)?;
                data.repository = query.repo.clone();
                for pr in &data.pull_requests {
                    positive(pr.number).map_err(invalid_json)?;
                }
                serde_json::to_value(data).unwrap()
            }
            "detail" => {
                let data: PullRequestDetail = decode(result)?;
                if Some(data.summary.number) != params["number"].as_u64() {
                    return Err(invalid_json("detail number differs from request"));
                }
                for file in &data.files {
                    safe_path(&file.path).map_err(invalid_json)?;
                }
                serde_json::to_value(data).unwrap()
            }
            "diff" => {
                let mut data: PullRequestDiff = decode(result)?;
                if Some(data.number) != params["number"].as_u64()
                    || Some(data.path.as_str()) != params["path"].as_str()
                {
                    return Err(invalid_json("diff identity differs from request"));
                }
                if let Some(path) = &data.original_path {
                    safe_path(path).map_err(invalid_json)?;
                }
                data.repository = query.repo.clone();
                serde_json::to_value(data).unwrap()
            }
            _ => return Err(PullRequestError::Invalid("unknown operation".into())),
        };
        let attach = |value: &mut Value| {
            value["provider"] = json!(target.provider);
            value["provider_name"] = json!(target.provider_name);
            value["remote"] = json!(target.remote);
        };
        attach(&mut result);
        if let Some(items) = result
            .get_mut("pull_requests")
            .and_then(Value::as_array_mut)
        {
            for item in items {
                attach(item);
            }
        }
        Ok(result)
    }
}
