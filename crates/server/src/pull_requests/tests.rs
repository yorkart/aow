use super::*;
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

use crate::AppState;
use aow_protocol::MyPullRequests;
fn paths() -> Vec<PathBuf> {
    vec!["/usr/local/bin".into(), "/usr/bin".into(), "/bin".into()]
}
fn provider(script: &str) -> Provider {
    Provider {
        id: "custom".into(),
        name: "Custom".into(),
        enabled: true,
        hosts: vec!["git.example.com".into()],
        script: script.into(),
    }
}
fn repository() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    let result = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    repo
}
fn remote(repo: &Path, name: &str, url: &str) {
    assert!(
        std::process::Command::new("git")
            .args(["remote", "add", name, url])
            .current_dir(repo)
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[test]
fn parses_remote_hosts_without_platform_assumptions() {
    for (raw, host, path) in [
        ("git@github.com:owner/repo.git", "github.com", "owner/repo"),
        (
            "ssh://git@git.example.com:2222/group/sub/repo.git",
            "git.example.com",
            "group/sub/repo",
        ),
        (
            "https://user:secret@Git.Example.com:8443/group/repo.git",
            "git.example.com:8443",
            "group/repo",
        ),
        ("git@work-alias:team/repo.git", "work-alias", "team/repo"),
    ] {
        assert_eq!(parse_remote(raw), Some((host.into(), path.into())));
    }
    for raw in [
        "/local/repo",
        "../repo",
        "file:///tmp/repo",
        "https://example.com",
        "C:/repo",
    ] {
        assert!(parse_remote(raw).is_none(), "{raw}");
    }
}
#[test]
fn defaults_scripts_edits_and_empty_config_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ProviderManager::persistent(dir.path()).unwrap();
    let initial = manager.settings().unwrap();
    assert_eq!(initial.providers.len(), 1);
    assert_eq!(initial.providers[0].id, "github");
    let mut next = initial.clone();
    next.providers[0].script = "print('customized')\n".into();
    next.providers[0].enabled = false;
    manager.save(next).unwrap();
    assert!(matches!(
        manager.save(initial),
        Err(PullRequestError::Conflict)
    ));
    let reloaded = ProviderManager::persistent(dir.path()).unwrap();
    let mut next = reloaded.settings().unwrap();
    assert!(!next.providers[0].enabled);
    assert_eq!(next.providers[0].script, "print('customized')\n");
    let config = reloaded.config.as_ref().unwrap();
    let document: Document =
        serde_json::from_slice(&std::fs::read(config.directory().join(CONFIG)).unwrap()).unwrap();
    assert_eq!(
        std::fs::read_to_string(config.directory().join(&document.providers[0].script_file))
            .unwrap(),
        next.providers[0].script
    );
    next.providers.clear();
    reloaded.save(next).unwrap();
    assert!(
        ProviderManager::persistent(dir.path())
            .unwrap()
            .settings()
            .unwrap()
            .providers
            .is_empty()
    );
}
#[test]
fn rejects_conflicting_hosts_and_path_ids() {
    let manager = ProviderManager::default();
    let mut next = manager.settings().unwrap();
    let mut second = next.providers[0].clone();
    second.id = "enterprise".into();
    next.providers.push(second);
    assert!(manager.save(next.clone()).is_err());
    next.providers[1].enabled = false;
    let mut next = manager.save(next).unwrap();
    next.providers[1].id = "../outside".into();
    assert!(manager.save(next).is_err());
    assert!(!managed_script_path("review-providers/../../outside.py"));
}

#[test]
fn bundled_protocol_upgrade_preserves_custom_scripts_and_provider_settings() {
    let dir = tempfile::tempdir().unwrap();
    let manager = ProviderManager::persistent(dir.path()).unwrap();
    let previous = "print('previous bundled adapter')\n";
    let digest = format!("{:x}", md5::compute(previous.as_bytes()));
    let mut settings = manager.settings().unwrap();
    settings.providers[0].script = previous.into();
    settings.providers[0].name = "Personal GitHub".into();
    settings.providers[0].enabled = false;
    let mut custom = settings.providers[0].clone();
    custom.id = "custom".into();
    custom.script.push_str("# local edits\n");
    settings.providers.push(custom.clone());
    let revision = settings.revision;
    assert!(upgrade_bundled_scripts(&mut settings, &["unrelated-digest", &digest]).unwrap());
    assert_eq!(settings.revision, revision + 1);
    assert_eq!(settings.providers[0].script, GITHUB_SCRIPT);
    assert_eq!(settings.providers[0].name, "Personal GitHub");
    assert!(!settings.providers[0].enabled);
    assert_eq!(settings.providers[1].script, custom.script);
    assert!(!upgrade_bundled_scripts(&mut settings, &["unrelated-digest", &digest]).unwrap());
    assert_eq!(settings.revision, revision + 1);
    manager.persist(&settings).unwrap();
    let restored = ProviderManager::persistent(dir.path())
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(restored.revision, settings.revision);
    assert_eq!(restored.providers[0].script, GITHUB_SCRIPT);
    assert_eq!(restored.providers[1].script, custom.script);
}

#[test]
fn bundled_branding_variant_upgrades_but_custom_logic_is_preserved() {
    let previous = "\"\"\"AoW review protocol v2 — GitHub adapter using gh.\"\"\"\nprint('old')\n";
    let digest = format!("{:x}", md5::compute(previous.as_bytes()));
    let mut settings = ProviderManager::default().settings().unwrap();
    settings.providers[0].script = previous.replacen("AoW", "AOW", 1);
    settings.providers[0].name = "Personal GitHub".into();
    let mut custom = settings.providers[0].clone();
    custom.id = "custom".into();
    custom.script.push_str("print('custom logic')\n");
    settings.providers.push(custom.clone());

    assert!(upgrade_bundled_scripts(&mut settings, &[&digest]).unwrap());
    assert_eq!(settings.revision, 1);
    assert_eq!(settings.providers[0].script, GITHUB_SCRIPT);
    assert_eq!(settings.providers[0].name, "Personal GitHub");
    assert_eq!(settings.providers[1].script, custom.script);
    assert!(!upgrade_bundled_scripts(&mut settings, &[&digest]).unwrap());
    assert_eq!(settings.revision, 1);
}
const FIXTURE: &str = r#"import json,sys,os
r=json.load(sys.stdin)
assert r['version']==2
assert set(r)=={'version','operation','repository','params'}
assert r['repository']['host']=='git.example.com'
assert os.path.realpath(r['repository']['root'])==os.path.realpath(os.getcwd())
p=r['params']
summary=dict(number=p.get('number',42),status='open',draft=False,title=r['repository']['path'],source_branch='feature',target_branch='main',url=None,created_at='',updated_at='')
if r['operation']=='list':
    items=[summary]
    if p.get('state')=='all':
        items += [dict(summary,number=43,status='merged'),dict(summary,number=44,status='closed')]
    result=dict(repository='ignored',current_branch='feature',current_user=dict(id='u',username='user',display_name='User'),pull_requests=items)
elif r['operation']=='detail':
    result=dict(summary,description='body',changes_count=1,commits_count=1,review_status='approved',check_summary_status='passed',mergeable=True,reviewers=[],checks=[],unresolved_threads=[],files=[dict(path='file.txt',change_type='M',additions=1,deletions=1)],author=None,labels=[],merge_checks=[],threads=[],warnings=[],diverged_commits_count=0,milestone=None)
else:
    result=dict(repository='ignored',number=p['number'],path=p['path'],original_path=None,original='before',modified='after',patch='@@ -1 +1 @@\n-before\n+after\n',binary=False,truncated=False)
print(json.dumps(dict(version=2,result=result)))
"#;
#[tokio::test]
async fn custom_provider_routes_all_operations_and_requires_unambiguous_remote() {
    let repo = repository();
    remote(repo.path(), "origin", "git@git.example.com:team/one.git");
    let manager = ProviderManager::default();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(FIXTURE)],
        })
        .unwrap();
    let mut query = ReviewQuery {
        repo: repo.path().to_string_lossy().into_owned(),
        provider: None,
        remote: None,
    };
    let result = manager
        .call(&query, "list", json!({}), &paths())
        .await
        .unwrap();
    assert_eq!(result["provider"], "custom");
    assert_eq!(result["repository"], query.repo);
    assert_eq!(result["pull_requests"][0]["remote"], "origin");
    // Exercise the public routes as well as the adapter so their names and
    // serialized response fields cannot drift away from the frontend API.
    let mut state = AppState::new(PathBuf::new());
    state.review_providers = manager.clone();
    let app = crate::build_router(state);
    for (suffix, filter) in [
        ("", ""),
        ("", "&state=open"),
        ("", "&state=all"),
        ("", "&state=invalid"),
        ("/42", ""),
        ("/42/diff", ""),
    ] {
        use axum::{
            body::{Body, to_bytes},
            http::Request,
        };
        use tower::ServiceExt;
        let response = app
            .clone()
            .oneshot(
                Request::get(format!(
                    "/api/my-pull-requests{suffix}?repo={}&path=file.txt{filter}",
                    query.repo
                ))
                .body(Body::empty())
                .unwrap(),
            )
            .await
            .unwrap();
        if filter == "&state=invalid" {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            continue;
        }
        assert_eq!(response.status(), StatusCode::OK, "{suffix}{filter}");
        let body = to_bytes(response.into_body(), OUTPUT_LIMIT).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["provider"], "custom");
        if suffix.is_empty() {
            assert_eq!(value["pull_requests"][0]["number"], 42);
            assert_eq!(value["pull_requests"][0]["remote"], "origin");
            let statuses: Vec<_> = value["pull_requests"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pr| pr["status"].as_str().unwrap())
                .collect();
            assert_eq!(
                statuses,
                if filter == "&state=all" {
                    vec!["open", "merged", "closed"]
                } else {
                    vec!["open"]
                }
            );
        } else {
            assert_eq!(value["number"], 42);
        }
    }
    let detail = manager
        .call(&query, "detail", json!({"number":42}), &paths())
        .await
        .unwrap();
    assert_eq!(detail["title"], "team/one");
    let diff = manager
        .call(
            &query,
            "diff",
            json!({"number":42,"path":"file.txt","patch_only":false}),
            &paths(),
        )
        .await
        .unwrap();
    assert_eq!(diff["modified"], "after");
    remote(
        repo.path(),
        "mirror",
        "https://git.example.com/team/one.git",
    );
    manager
        .call(&query, "list", json!({}), &paths())
        .await
        .unwrap();
    remote(
        repo.path(),
        "upstream",
        "https://git.example.com/team/two.git",
    );
    assert!(matches!(
        manager.call(&query, "list", json!({}), &paths()).await,
        Err(PullRequestError::Invalid(_))
    ));
    query.remote = Some("upstream".into());
    query.provider = Some("custom".into());
    assert_eq!(
        manager
            .call(&query, "list", json!({}), &paths())
            .await
            .unwrap()["pull_requests"][0]["title"],
        "team/two"
    );
    query.provider = Some("missing".into());
    assert!(matches!(
        manager.call(&query, "list", json!({}), &paths()).await,
        Err(PullRequestError::Unavailable(_))
    ));
}
#[tokio::test]
async fn managed_script_executes_after_restart() {
    let repo = repository();
    remote(repo.path(), "origin", "git@git.example.com:team/one.git");
    let state = tempfile::tempdir().unwrap();
    let manager = ProviderManager::persistent(state.path()).unwrap();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(FIXTURE)],
        })
        .unwrap();
    let manager = ProviderManager::persistent(state.path()).unwrap();
    let query = ReviewQuery {
        repo: repo.path().to_string_lossy().into_owned(),
        provider: None,
        remote: None,
    };
    assert_eq!(
        manager
            .call(&query, "list", json!({}), &paths())
            .await
            .unwrap()["provider"],
        "custom"
    );
}

const LINKS_FIXTURE: &str = r#"import json,sys,os
r=json.load(sys.stdin)
assert r['version']==2
assert 'secret' not in json.dumps(r)
if r['operation']=='describe':
    assert r['repository'] is None
    result={'operations':['list','detail','diff','commit_links']}
else:
    assert r['operation']=='commit_links'
    assert r['repository']==dict(root=os.getcwd(),host='git.example.com',path='team/nested/project',remote='review')
    sha=r['params']['commit']
    assert len(sha)==40 and all(c in '0123456789abcdef' for c in sha)
    result={'remote_url':'https://browser.example.org/project/123',
            'commit_url':'https://browser.example.org/project/123/revisions/'+sha}
print(json.dumps(dict(version=2,result=result)))
"#;

const AVATAR_FIXTURE: &str = r#"import json,sys,os
r=json.load(sys.stdin)
assert 'secret' not in json.dumps(r)
if r['operation']=='describe':
    assert r['repository'] is None
    result={'operations':['list','detail','diff','repository_info']}
else:
    assert r['operation']=='repository_info'
    assert r['params']=={}
    assert os.path.realpath(r['repository']['root'])==os.path.realpath(os.getcwd())
    with open('.avatar-calls','a') as log: log.write('call\n')
    result={'avatar_url':'https://avatars.example.com/'+r['repository']['path'].split('/')[0]}
print(json.dumps(dict(version=2,result=result)))
"#;

#[tokio::test]
async fn repository_avatars_use_origin_and_cache_by_remote_provider_and_path() {
    let repo = repository();
    let root = repo.path().canonicalize().unwrap();
    let repo_path = root.to_str().unwrap();
    remote(
        &root,
        "origin",
        "https://user:secret@git.example.com/team/repo.git",
    );
    remote(&root, "upstream", "git@git.example.com:other/repo.git");
    let manager = ProviderManager::default();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(AVATAR_FIXTURE)],
        })
        .unwrap();
    let paths = paths();
    let (first, second) = tokio::join!(
        manager.repository_avatar(repo_path, &paths),
        manager.repository_avatar(repo_path, &paths)
    );
    assert_eq!(
        first.unwrap().as_deref(),
        Some("https://avatars.example.com/team")
    );
    assert_eq!(
        second.unwrap().as_deref(),
        Some("https://avatars.example.com/team")
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".avatar-calls")).unwrap(),
        "call\n"
    );

    git(
        repo_path,
        &[
            "remote",
            "set-url",
            "origin",
            "git@git.example.com:new/repo.git",
        ],
        &paths,
    )
    .await
    .unwrap();
    assert_eq!(
        manager
            .repository_avatar(repo_path, &paths)
            .await
            .unwrap()
            .as_deref(),
        Some("https://avatars.example.com/new")
    );
    let mut next_paths = paths.clone();
    next_paths.push("/extra-bin".into());
    manager
        .repository_avatar(repo_path, &next_paths)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join(".avatar-calls"))
            .unwrap()
            .lines()
            .count(),
        3
    );

    let mut settings = manager.settings().unwrap();
    settings.providers[0].script = AVATAR_FIXTURE.replace(
        "'https://avatars.example.com/'+r['repository']['path'].split('/')[0]",
        "None",
    );
    manager.save(settings).unwrap();
    assert!(
        manager
            .repository_avatar(repo_path, &paths)
            .await
            .unwrap()
            .is_none()
    );
    // An unmatched origin must not fall through to a configured upstream.
    git(
        repo_path,
        &[
            "remote",
            "set-url",
            "origin",
            "git@unknown.example.com:team/repo.git",
        ],
        &paths,
    )
    .await
    .unwrap();
    assert!(
        manager
            .repository_avatar(repo_path, &paths)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn repository_avatars_support_a_single_remote_and_skip_legacy_or_ambiguous_providers() {
    let repo = repository();
    let root = repo.path().canonicalize().unwrap();
    let repo_path = root.to_str().unwrap();
    let manager = ProviderManager::default();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(AVATAR_FIXTURE)],
        })
        .unwrap();
    assert!(
        manager
            .repository_avatar(repo_path, &paths())
            .await
            .unwrap()
            .is_none()
    );
    remote(&root, "review", "git@git.example.com:team/repo.git");
    assert_eq!(
        manager
            .repository_avatar(repo_path, &paths())
            .await
            .unwrap()
            .as_deref(),
        Some("https://avatars.example.com/team")
    );
    remote(&root, "upstream", "git@git.example.com:other/repo.git");
    assert!(
        manager
            .repository_avatar(repo_path, &paths())
            .await
            .unwrap()
            .is_none()
    );
    git(repo_path, &["remote", "remove", "upstream"], &paths())
        .await
        .unwrap();

    let mut settings = manager.settings().unwrap();
    settings.providers[0].script = "import json,sys\nr=json.load(sys.stdin)\nassert r['operation']=='describe'\nprint(json.dumps({'version':2,'result':{'operations':['list','detail','diff']}}))".into();
    manager.save(settings).unwrap();
    assert!(
        manager
            .repository_avatar(repo_path, &paths())
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".avatar-calls")).unwrap(),
        "call\n"
    );
}

#[tokio::test]
async fn repository_avatars_reject_invalid_urls_and_recover_after_provider_changes() {
    let repo = repository();
    let root = repo.path().canonicalize().unwrap();
    let repo_path = root.to_str().unwrap();
    remote(&root, "origin", "git@git.example.com:team/repo.git");
    let manager = ProviderManager::default();
    for url in [
        "javascript:alert(1)",
        "file:///tmp/icon.png",
        "/relative",
        "https://user:secret@example.com/icon",
        "https://example.com/icon\n",
    ] {
        let mut settings = manager.settings().unwrap();
        settings.providers = vec![provider(&format!(
            "import json,sys\nr=json.load(sys.stdin)\nv={{'operations':['list','detail','diff','repository_info']}} if r['operation']=='describe' else {{'avatar_url':{}}}\nprint(json.dumps({{'version':2,'result':v}}))",
            serde_json::to_string(url).unwrap()
        ))];
        manager.save(settings).unwrap();
        assert!(
            manager
                .repository_avatar(repo_path, &paths())
                .await
                .is_err(),
            "{url}"
        );
    }
    let mut settings = manager.settings().unwrap();
    settings.providers = vec![provider(AVATAR_FIXTURE)];
    manager.save(settings).unwrap();
    assert_eq!(
        manager
            .repository_avatar(repo_path, &paths())
            .await
            .unwrap()
            .as_deref(),
        Some("https://avatars.example.com/team")
    );
    let mut settings = manager.settings().unwrap();
    settings.providers[0].enabled = false;
    manager.save(settings).unwrap();
    assert!(
        manager
            .repository_avatar(repo_path, &paths())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn commit_links_follow_configured_provider_and_preserve_local_details() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    let repo = repository();
    let root = repo.path().canonicalize().unwrap();
    assert!(
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Example",
                "-c",
                "user.email=example@example.com",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "Local commit"
            ])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    remote(
        &root,
        "origin",
        "git@unconfigured.example.com:other/project.git",
    );
    remote(
        &root,
        "review",
        "ssh://user:secret@git.example.com:2222/team/nested/project.git",
    );
    let branch = git(
        root.to_str().unwrap(),
        &["branch", "--show-current"],
        &paths(),
    )
    .await
    .unwrap();
    git(
        root.to_str().unwrap(),
        &[
            "config",
            &format!("branch.{}.remote", branch.trim()),
            "review",
        ],
        &paths(),
    )
    .await
    .unwrap();
    let sha = git(root.to_str().unwrap(), &["rev-parse", "HEAD"], &paths())
        .await
        .unwrap();
    let sha = sha.trim();
    let manager = ProviderManager::default();
    let mut state = AppState::new(PathBuf::new());
    state.review_providers = manager.clone();
    let app = crate::build_router(state);
    let url = format!(
        "/api/git/commit/detail?repo={}&commit={}",
        root.display(),
        &sha[..10]
    );

    for (script, enabled, has_links) in [
        (LINKS_FIXTURE, true, true),
        (LINKS_FIXTURE, false, false),
        (
            "import json;print(json.dumps({'version':2,'result':{'operations':['list','detail','diff']}}))",
            true,
            false,
        ),
        ("raise RuntimeError('Provider failed')", true, false),
        ("import time;time.sleep(60)", true, false),
    ] {
        let mut settings = manager.settings().unwrap();
        settings.providers = vec![Provider {
            enabled,
            ..provider(script)
        }];
        manager.save(settings).unwrap();
        let response = app
            .clone()
            .oneshot(Request::get(&url).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), OUTPUT_LIMIT).await.unwrap())
                .unwrap();
        assert_eq!(value["id"], sha);
        assert_eq!(value["subject"], "Local commit");
        assert_eq!(value["remote_name"], "review");
        if has_links {
            assert_eq!(
                value["remote_url"],
                "https://browser.example.org/project/123"
            );
            assert_eq!(
                value["commit_url"],
                format!("https://browser.example.org/project/123/revisions/{sha}")
            );
        } else {
            assert!(value["remote_url"].is_null());
            assert!(value["commit_url"].is_null());
        }
    }

    let mut settings = manager.settings().unwrap();
    settings.providers = vec![provider(LINKS_FIXTURE)];
    manager.save(settings).unwrap();
    // A configured provider for a different remote must not replace the
    // local branch's selected remote or invent a URL for it.
    assert!(
        manager
            .commit_links(root.to_str().unwrap(), "origin", sha, &paths())
            .await
            .unwrap()
            .is_none()
    );
    let mut settings = manager.settings().unwrap();
    settings.providers.clear();
    manager.save(settings).unwrap();
    assert!(
        manager
            .commit_links(root.to_str().unwrap(), "review", sha, &paths())
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn commit_links_accept_only_explicit_web_urls_or_null() {
    for url in [
        "https://web.example.com/project/revision/abc",
        "http://localhost:8080/changes/abc",
    ] {
        assert_eq!(
            web_link(&json!({"commit_url":url}), "commit_url")
                .unwrap()
                .as_deref(),
            Some(url)
        );
    }
    assert!(
        web_link(&json!({"commit_url":null}), "commit_url")
            .unwrap()
            .is_none()
    );
    for value in [
        json!({}),
        json!({"commit_url":12}),
        json!({"commit_url":"javascript:alert(1)"}),
        json!({"commit_url":"/relative/path"}),
        json!({"commit_url":"https://user:secret@web.example.com/commit"}),
        json!({"commit_url":"https://web.example.com/commit\n"}),
    ] {
        assert!(web_link(&value, "commit_url").is_err(), "{value}");
    }
}

#[tokio::test]
async fn validates_envelope_exit_code_schema_and_process_limits() {
    let request = json!({"version":2,"operation":"describe","params":{}});
    for script in [
        "print('not-json')",
        "print('{\"version\":1,\"result\":{}}')",
        "print('{\"version\":2,\"result\":[],\"error\":{}}')",
        "import sys;sys.exit(1)",
        "print('{\"version\":2,\"error\":{\"code\":\"auth\",\"message\":\"login required\"}}')",
    ] {
        assert!(
            run_adapter(&provider(script), request.clone(), None, &paths(), None)
                .await
                .is_err(),
            "{script}"
        );
    }
    assert!(decode::<MyPullRequests>(json!({"pull_requests":[]})).is_err());
    assert!(
        decode::<MyPullRequests>(json!({
            "repository":"/repo", "current_branch":"main",
            "current_user":{"id":"u","username":"user","display_name":"User"}
        }))
        .is_err()
    );
    assert!(safe_path("../outside").is_err());
    assert!(positive(0).is_err());
    let mut command = Command::new("/bin/sleep");
    command.arg("10");
    assert!(matches!(
        run_command(command, &[], &paths(), Duration::from_millis(30)).await,
        Err(PullRequestError::Timeout)
    ));
    let reader = &b"12345"[..];
    assert!(read_limited(reader, 4).await.is_err());
}
#[tokio::test]
async fn settings_and_draft_validation_http_contract() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let app = crate::build_router(AppState::new(PathBuf::new()));
    let response = app
        .clone()
        .oneshot(
            Request::get("/api/aow/review-providers")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 2 * SCRIPT_LIMIT)
        .await
        .unwrap();
    let mut settings: Settings = serde_json::from_slice(&body).unwrap();
    let settings_json: Value = serde_json::from_slice(&body).unwrap();
    assert!(settings_json["providers"][0].get("cli").is_none());
    let mut draft = settings.providers[0].clone();
    draft.script = r#"import json,sys
r=json.load(sys.stdin)
assert set(r)=={'version','operation','repository','params'}
assert r['operation']=='describe' and r['repository'] is None
print(json.dumps(dict(version=2,result=dict(operations=['list','detail','diff']))))
"#
    .into();
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/aow/review-providers/test")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&draft).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    settings.providers.clear();
    let body = serde_json::to_vec(&settings).unwrap();
    for status in [StatusCode::OK, StatusCode::CONFLICT] {
        let response = app
            .clone()
            .oneshot(
                Request::put("/api/aow/review-providers")
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
    }
}

#[tokio::test]
async fn issues_require_declared_capability_without_breaking_prs() {
    let repo = repository();
    remote(repo.path(), "origin", "git@git.example.com:team/repo.git");
    let manager = ProviderManager::default();
    let script = "import json,sys\nr=json.load(sys.stdin)\nassert r['operation']=='describe'\nprint(json.dumps({'version':2,'result':{'operations':['list','detail','diff']}}))";
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(script)],
        })
        .unwrap();
    let query = ReviewQuery {
        repo: repo.path().to_string_lossy().into_owned(),
        provider: None,
        remote: None,
    };
    assert!(matches!(
        manager.call(&query, "issues", json!({}), &paths()).await,
        Err(PullRequestError::Unavailable(_))
    ));
}

#[test]
fn issue_provider_rejects_unsafe_links_duplicate_ids_and_bad_labels() {
    let issue = json!({"number":42,"title":"Issue","status":"open","url":"https://example.com/issues/42","labels":[],"assignees":[],"updated_at":"2026-09-29T00:00:00Z"});
    assert!(issues::validate("issues", json!({"issues":[issue.clone()]})).is_ok());
    assert!(issues::validate("issues", json!({"issues":[issue.clone(),issue.clone()]})).is_err());
    for (key, value) in [
        ("url", json!("javascript:alert(1)")),
        ("url", json!("https://user:secret@example.com/issue")),
        ("status", json!("merged")),
        ("number", json!(0)),
        ("updated_at", json!("invalid")),
    ] {
        let mut invalid = issue.clone();
        invalid[key] = value;
        assert!(issues::validate("issues", json!({"issues":[invalid]})).is_err());
    }
    assert!(
        issues::validate(
            "issue_labels",
            json!({"labels":[{"name":"bug","color":"nope","description":""}]})
        )
        .is_err()
    );
}
