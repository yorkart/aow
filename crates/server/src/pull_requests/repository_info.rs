use super::*;
use std::{collections::HashMap, time::Instant};

type AvatarResult = std::result::Result<Option<String>, String>;
type AvatarEntry = Arc<tokio::sync::Mutex<Option<CachedAvatar>>>;

#[derive(Default)]
pub(super) struct RepositoryInfoCache {
    entries: Mutex<HashMap<String, AvatarEntry>>,
}

struct CachedAvatar {
    source: String,
    expires: Instant,
    result: AvatarResult,
}

impl ProviderManager {
    /// Optional repository metadata, independent of PR lists and branch state.
    pub async fn repository_avatar(&self, repo: &str, paths: &[PathBuf]) -> Result<Option<String>> {
        timeout(
            Duration::from_secs(10),
            self.repository_avatar_inner(repo, paths),
        )
        .await
        .map_err(|_| PullRequestError::Timeout)?
    }

    async fn repository_avatar_inner(
        &self,
        repo: &str,
        paths: &[PathBuf],
    ) -> Result<Option<String>> {
        absolute(repo)?;
        let settings = self.settings()?;
        let names = git(repo, &["remote"], paths).await?;
        let names: Vec<_> = names.lines().filter(|name| !name.is_empty()).collect();
        // Never borrow an upstream's identity when origin has no matching
        // provider. Without origin, only a single remote is unambiguous.
        let remote = if names.contains(&"origin") {
            "origin"
        } else if names.len() == 1 {
            names[0]
        } else {
            return Ok(None);
        };
        let url = git(repo, &["remote", "get-url", "--", remote], paths).await?;
        let Some((host, path)) = parse_remote(url.trim()) else {
            return Ok(None);
        };
        let Some(provider) = settings
            .providers
            .iter()
            .find(|p| p.enabled && p.hosts.contains(&host))
        else {
            return Ok(None);
        };
        let source = json!([settings.revision, remote, host, path, paths]).to_string();
        let entry = self
            .repository_info
            .entries
            .lock()
            .map_err(|_| PullRequestError::Command("Repository info cache lock failed".into()))?
            .entry(repo.to_owned())
            .or_default()
            .clone();
        // Serialize only calls for this repository; other projects load freely.
        let mut cached = entry.lock().await;
        if let Some(value) = cached
            .as_ref()
            .filter(|value| value.source == source && value.expires > Instant::now())
        {
            return value.result.clone().map_err(PullRequestError::Command);
        }
        let request = async {
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
                .any(|op| op.as_str() == Some("repository_info"))
            {
                return Ok(None);
            }
            let result = self.run_provider(provider,
                json!({"version":2,"operation":"repository_info",
                    "repository":{"root":repo,"host":host,"path":path,"remote":remote},"params":{}}),
                Some(Path::new(repo)), paths).await?;
            web_link(&result, "avatar_url")
        };
        let result = timeout(Duration::from_secs(8), request)
            .await
            .map_err(|_| PullRequestError::Timeout)
            .and_then(|result| result)
            .map_err(|error| error.to_string());
        let ttl = if matches!(result, Ok(Some(_))) {
            15 * 60
        } else {
            60
        };
        *cached = Some(CachedAvatar {
            source,
            expires: Instant::now() + Duration::from_secs(ttl),
            result: result.clone(),
        });
        result.map_err(PullRequestError::Command)
    }
}
