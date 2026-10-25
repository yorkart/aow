use super::*;
use crate::AppState;
use crate::pull_requests::{decode, positive, safe_path};
use aow_protocol::MyPullRequests;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;
use tower::ServiceExt;

fn paths() -> Vec<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect()
}
fn provider(script: &str) -> GitPlatformProvider {
    GitPlatformProvider {
        id: "custom.mjs".into(),
        name: "Custom".into(),
        hosts: vec!["git.example.com".into()],
        script: script.into(),
    }
}
fn repository() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    repo
}
fn remote(repo: &Path, name: &str, url: &str) {
    assert!(
        std::process::Command::new("git")
            .args(["remote", "add", name, url])
            .current_dir(repo)
            .status()
            .unwrap()
            .success()
    );
}
const FIXTURE: &str = r#"import {readFileSync, appendFileSync} from 'node:fs';
import assert from 'node:assert/strict';
const r=JSON.parse(readFileSync(0,'utf8'));
assert.equal(r.version,4);
assert.deepEqual(Object.keys(r).sort(),['operation','params','repository','version']);
const p=r.params;
const summary={number:p.number??42,status:'open',draft:false,title:r.repository?.path??'',source_branch:'feature',target_branch:'main',url:null,created_at:'',updated_at:''};
let result;
switch(r.operation) {
case 'describe':
  assert.equal(r.repository,null);
  result={name:'Custom',hosts:['git.example.com'],operations:['pull_requests.list','pull_requests.detail','pull_requests.diff','repository.commit_links','repository.info']}; break;
case 'pull_requests.list':
  result={repository:'ignored',current_branch:'feature',current_user:{id:'u',username:'user',display_name:'User'},pull_requests:[summary]};
  if(p.state==='all') result.pull_requests.push({...summary,number:43,status:'merged'},{...summary,number:44,status:'closed'}); break;
case 'pull_requests.detail': result={...summary,description:'body',changes_count:1,commits_count:1,review_status:'approved',check_summary_status:'passed',mergeable:true,reviewers:[],checks:[],unresolved_threads:[],files:[{path:'file.txt',change_type:'M',additions:1,deletions:1}],author:null,labels:[],merge_checks:[],threads:[],warnings:[],diverged_commits_count:0,milestone:null}; break;
case 'pull_requests.diff': result={repository:'ignored',number:p.number,path:p.path,original_path:null,original:p.patch_only?null:'before',modified:p.patch_only?null:'after',patch:'@@ -1 +1 @@\n-before\n+after\n',binary:false,truncated:false}; break;
case 'repository.commit_links': result={remote_url:'https://browser.example.org/project',commit_url:'https://browser.example.org/commit/'+p.commit}; break;
case 'repository.info': appendFileSync('.avatar-calls','call\n'); result={avatar_url:'https://avatars.example.com/'+r.repository.path.split('/')[0]}; break;
default: throw new Error('unexpected operation');
}
process.stdout.write(JSON.stringify({version:4,result}));
"#;

#[test]
fn parses_remote_hosts_and_checks_original_filenames() {
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
        assert!(parse_remote(raw).is_none());
    }
    for name in ["github.mjs", "My-company.v2.mjs", "gitlab.mjs"] {
        assert!(validation::valid_filename(name));
    }
    for name in [
        "../outside.mjs",
        "a/b.mjs",
        "a\\b.mjs",
        "a.py",
        "a.js",
        "a.ts",
        ".mjs",
        "a.mjs\n",
        "a.MJS",
    ] {
        assert!(!validation::valid_filename(name), "{name}");
    }
}

#[tokio::test]
async fn repository_only_providers_load_and_consumers_select_declared_capabilities() {
    let state = tempfile::tempdir().unwrap();
    let script = FIXTURE.replace(
        "operations:['pull_requests.list','pull_requests.detail','pull_requests.diff','repository.commit_links','repository.info']",
        "operations:['repository.info']",
    );
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![provider(&script)])
            .unwrap();
    assert_eq!(manager.settings().unwrap().providers.len(), 1);
    let repo = repository();
    remote(
        repo.path(),
        "origin",
        "git@git.example.com:team/project.git",
    );
    let root = repo.path().to_str().unwrap();
    assert_eq!(manager.targets(root, &[], &paths()).await.unwrap().len(), 1);
    assert_eq!(
        manager
            .targets(root, &["repository.info"], &paths())
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        manager
            .targets(
                root,
                &[
                    "pull_requests.list",
                    "pull_requests.detail",
                    "pull_requests.diff"
                ],
                &paths()
            )
            .await
            .unwrap()
            .is_empty()
    );
    let query = GitPlatformQuery {
        repo: root.into(),
        provider: None,
        remote: None,
    };
    let invocation = manager
        .call(&query, "repository.info", json!({}), &paths())
        .await
        .unwrap();
    assert_eq!(
        invocation.result["avatar_url"],
        "https://avatars.example.com/team"
    );
    assert!(matches!(
        manager
            .call(&query, "pull_requests.list", json!({}), &paths())
            .await,
        Err(GitPlatformError::Unavailable(_))
    ));
    for operation in [
        "list",
        "detail",
        "diff",
        "repository_info",
        "commit_links",
        "pull_requests.merge",
    ] {
        assert!(
            manager
                .call(&query, operation, json!({}), &paths())
                .await
                .is_err()
        );
        assert!(manager.targets(root, &[operation], &paths()).await.is_err());
    }
}

#[test]
fn original_filenames_persist_and_default_updates_preserve_customization_and_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let default = provider(FIXTURE);
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(dir.path(), vec![default.clone()])
            .unwrap();
    let initial = manager.settings().unwrap();
    let file = manager
        .config
        .as_ref()
        .unwrap()
        .directory()
        .join("git-platform-providers/custom.mjs");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), FIXTURE);
    let doc: Value = serde_json::from_slice(
        &std::fs::read(manager.config.as_ref().unwrap().directory().join(CONFIG)).unwrap(),
    )
    .unwrap();
    assert!(doc["providers"][0].get("script_file").is_none());
    assert!(doc["providers"][0].get("enabled").is_none());
    manager.save(initial.clone()).unwrap();
    assert!(matches!(
        manager.save(initial),
        Err(GitPlatformError::Conflict)
    ));
    let updated = GitPlatformProvider {
        script: format!("{FIXTURE}\n// release update"),
        ..default.clone()
    };
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(dir.path(), vec![updated.clone()])
            .unwrap();
    let mut next = manager.settings().unwrap();
    assert_eq!(next.providers[0].script, updated.script);
    next.providers[0].script.push_str("\n// user edit");
    let custom = next.providers[0].script.clone();
    manager.save(next).unwrap();
    let later = GitPlatformProvider {
        script: format!("{FIXTURE}\n// later release"),
        ..default
    };
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(dir.path(), vec![later.clone()])
            .unwrap();
    let mut next = manager.settings().unwrap();
    assert_eq!(next.providers[0].script, custom);
    next.providers.clear();
    manager.save(next).unwrap();
    assert!(!file.exists());
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(dir.path(), vec![later]).unwrap();
    assert!(manager.settings().unwrap().providers.is_empty());
}

#[test]
fn unsupported_config_versions_are_rejected_without_rewriting_files() {
    let dir = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigStore::initialize(dir.path()).unwrap();
    for version in [1, 2, 4] {
        let bytes = serde_json::to_vec(
            &json!({"version":version,"revision":4,"providers":[],"bundled":{}}),
        )
        .unwrap();
        config.save(Path::new(CONFIG), &bytes).unwrap();
        let error = GitPlatformProviderManager::persistent_with_defaults(
            dir.path(),
            vec![provider(FIXTURE)],
        )
        .err()
        .expect("unsupported config version must be rejected");
        assert!(
            error
                .to_string()
                .contains("unsupported Git platform provider config version")
        );
        assert_eq!(
            std::fs::read(config.directory().join(CONFIG)).unwrap(),
            bytes
        );
        assert!(!config.directory().join("git-platform-providers").exists());
    }
}

#[test]
fn duplicate_domains_and_case_colliding_filenames_are_rejected() {
    let first = provider(FIXTURE);
    let mut second = first.clone();
    second.id = "other.mjs".into();
    let mut settings = Settings {
        revision: 0,
        providers: vec![first, second],
    };
    assert!(validate_settings(&settings).is_err());
    settings.providers[1].hosts = vec!["other.example.com".into()];
    assert!(validate_settings(&settings).is_ok());
    settings.providers[1].id = "CUSTOM.mjs".into();
    assert!(validate_settings(&settings).is_err());
}

#[tokio::test]
async fn custom_node_provider_routes_reads_and_requires_unambiguous_remote() {
    let repo = repository();
    remote(repo.path(), "origin", "git@git.example.com:team/one.git");
    let manager = GitPlatformProviderManager::default();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(FIXTURE)],
        })
        .unwrap();
    let mut query = GitPlatformQuery {
        repo: repo.path().to_string_lossy().into_owned(),
        provider: None,
        remote: None,
    };
    let listed = crate::pull_requests::call(
        &manager,
        &query,
        "pull_requests.list",
        json!({"state":"all"}),
        &paths(),
    )
    .await
    .unwrap();
    assert_eq!(listed["provider"], "custom.mjs");
    assert_eq!(listed["repository"], query.repo);
    assert_eq!(listed["pull_requests"].as_array().unwrap().len(), 3);
    assert_eq!(
        crate::pull_requests::call(
            &manager,
            &query,
            "pull_requests.detail",
            json!({"number":42}),
            &paths()
        )
        .await
        .unwrap()["number"],
        42
    );
    let diff = crate::pull_requests::call(
        &manager,
        &query,
        "pull_requests.diff",
        json!({"number":42,"path":"file.txt","patch_only":true}),
        &paths(),
    )
    .await
    .unwrap();
    assert!(diff["original"].is_null());
    assert!(diff["patch"].as_str().unwrap().contains("+after"));
    remote(repo.path(), "upstream", "git@git.example.com:team/two.git");
    assert!(
        crate::pull_requests::call(&manager, &query, "pull_requests.list", json!({}), &paths())
            .await
            .is_err()
    );
    query.remote = Some("upstream".into());
    assert_eq!(
        crate::pull_requests::call(&manager, &query, "pull_requests.list", json!({}), &paths())
            .await
            .unwrap()["pull_requests"][0]["title"],
        "team/two"
    );
    assert!(
        crate::pull_requests::call(&manager, &query, "merge", json!({"number":42}), &paths())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn identity_mismatches_and_unsafe_paths_fail_after_node_execution() {
    let repo = repository();
    remote(repo.path(), "origin", "git@git.example.com:team/one.git");
    let query = GitPlatformQuery {
        repo: repo.path().to_string_lossy().into_owned(),
        provider: None,
        remote: None,
    };
    for (script, operation, params) in [
        (
            FIXTURE.replace("number:p.number??42", "number:99"),
            "pull_requests.detail",
            json!({"number":42}),
        ),
        (
            FIXTURE.replace("path:p.path", "path:'../outside'"),
            "pull_requests.diff",
            json!({"number":42,"path":"file.txt"}),
        ),
        (
            FIXTURE.replace("path:'file.txt'", "path:'../outside'"),
            "pull_requests.detail",
            json!({"number":42}),
        ),
    ] {
        let manager = GitPlatformProviderManager::default();
        manager
            .save(Settings {
                revision: 0,
                providers: vec![provider(&script)],
            })
            .unwrap();
        assert!(
            crate::pull_requests::call(&manager, &query, operation, params, &paths())
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn repository_avatar_cache_and_commit_links_remain_provider_owned() {
    let repo = repository();
    remote(
        repo.path(),
        "origin",
        "https://user:secret@git.example.com/team/project.git",
    );
    let manager = GitPlatformProviderManager::default();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(FIXTURE)],
        })
        .unwrap();
    let root = repo.path().to_str().unwrap();
    for _ in 0..2 {
        assert_eq!(
            manager
                .repository_avatar(root, &paths())
                .await
                .unwrap()
                .as_deref(),
            Some("https://avatars.example.com/team")
        );
    }
    assert_eq!(
        std::fs::read_to_string(repo.path().join(".avatar-calls")).unwrap(),
        "call\n"
    );
    let links = manager
        .commit_links(root, "origin", &"a".repeat(40), &paths())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        links.remote_url.as_deref(),
        Some("https://browser.example.org/project")
    );
    assert!(links.commit_url.unwrap().ends_with(&"a".repeat(40)));
    for value in [
        json!({"url":"javascript:alert(1)"}),
        json!({"url":"https://user:secret@example.com"}),
        json!({"url":"https://example.com/\n"}),
    ] {
        assert!(web_link(&value, "url").is_err());
    }
}

#[tokio::test]
async fn validates_metadata_envelopes_process_limits_and_read_only_operations() {
    let request = json!({"version":4,"operation":"describe","repository":null,"params":{}});
    for script in [
        "console.log('not json')",
        "console.log(JSON.stringify({version:2,result:{}}))",
        "process.exit(1)",
        "console.log(JSON.stringify({version:4,result:{},error:{code:'bad',message:'error'}}))",
        "console.log(JSON.stringify({version:4,error:{code:'auth_required',message:'login required'}}))",
    ] {
        assert!(
            run_adapter(&provider(script), request.clone(), None, &paths(), None)
                .await
                .is_err()
        );
    }
    assert!(
        description::describe(
            &provider(&FIXTURE.replace("'repository.info']", "'merge']")),
            &paths()
        )
        .await
        .is_err()
    );
    assert!(description::describe(&provider("console.log(JSON.stringify({version:4,result:{operations:['pull_requests.list','pull_requests.detail','pull_requests.diff']}}))"), &paths()).await.is_err());
    assert!(decode::<MyPullRequests>(json!({"pull_requests":[]})).is_err());
    assert!(safe_path("../outside").is_err());
    assert!(positive(0).is_err());
    let mut command = Command::new("/bin/sleep");
    command.arg("10");
    assert!(matches!(
        run_command(command, &[], &paths(), Duration::from_millis(30)).await,
        Err(GitPlatformError::Timeout)
    ));
    assert!(read_limited(&b"12345"[..], 4).await.is_err());
}

#[tokio::test]
async fn uploads_activate_immediately_and_removal_stops_routing_without_enable_flags() {
    let state = AppState::new(PathBuf::new());
    let manager = state.git_platform_providers.clone();
    let app = crate::build_router(state);
    let repo = repository();
    remote(repo.path(), "origin", "git@git.example.com:team/one.git");
    let root = repo.path().to_str().unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::post("/api/aow/git-platform-providers/test")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"id":"custom.mjs","script":FIXTURE}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), SCRIPT_LIMIT).await.unwrap())
            .unwrap();
    assert_eq!(value["name"], "Custom");
    let mut p = provider(FIXTURE);
    p.name = "Forged name".into();
    p.hosts = vec!["forged.example".into()];
    let body = serde_json::to_vec(&Settings {
        revision: 0,
        providers: vec![p],
    })
    .unwrap();
    for status in [StatusCode::OK, StatusCode::CONFLICT] {
        let response = app
            .clone()
            .oneshot(
                Request::put("/api/aow/git-platform-providers")
                    .header("content-type", "application/json")
                    .body(Body::from(body.clone()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == StatusCode::OK {
            let value: Value = serde_json::from_slice(
                &to_bytes(response.into_body(), SCRIPT_LIMIT * 2)
                    .await
                    .unwrap(),
            )
            .unwrap();
            assert!(value["providers"][0].get("enabled").is_none());
            assert_eq!(value["providers"][0]["name"], "Custom");
            assert_eq!(value["providers"][0]["hosts"], json!(["git.example.com"]));
        }
    }
    assert_eq!(manager.targets(root, &[], &paths()).await.unwrap().len(), 1);
    assert!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .is_some()
    );
    let saved = manager.settings().unwrap();
    let mut failed_upload = saved.clone();
    failed_upload.providers[0].script = "console.log('invalid')".into();
    let mut toggle = serde_json::to_value(&saved).unwrap();
    toggle["providers"][0]["enabled"] = json!(false);
    for invalid in [serde_json::to_value(failed_upload).unwrap(), toggle] {
        let response = app
            .clone()
            .oneshot(
                Request::put("/api/aow/git-platform-providers")
                    .header("content-type", "application/json")
                    .body(Body::from(invalid.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.status().is_success());
        assert_eq!(manager.settings().unwrap().revision, saved.revision);
        assert_eq!(manager.settings().unwrap().providers[0].script, FIXTURE);
    }
    let response = app
        .clone()
        .oneshot(
            Request::put("/api/aow/git-platform-providers")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"revision":saved.revision,"providers":[]}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        manager
            .targets(root, &[], &paths())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .is_none()
    );
    let response = app
        .oneshot(
            Request::post("/api/aow/git-platform-providers/test")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"id":"custom.py","script":FIXTURE}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(!response.status().is_success());
}

#[test]
fn python_providers_are_rejected() {
    let manager = GitPlatformProviderManager::default();
    let settings = Settings {
        revision: 0,
        providers: vec![GitPlatformProvider {
            id: "custom.py".into(),
            ..provider(FIXTURE)
        }],
    };
    assert!(matches!(
        manager.save(settings),
        Err(GitPlatformError::Invalid(_))
    ));
    assert!(manager.settings().unwrap().providers.is_empty());
}

#[tokio::test]
async fn saved_provider_runs_its_original_filename_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let script = format!(
        "assert.ok(process.argv[1].endsWith('/git-platform-providers/custom.mjs'));\n{FIXTURE}"
    );
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(dir.path(), vec![provider(&script)])
            .unwrap();
    drop(manager);
    let manager = GitPlatformProviderManager::persistent_with_defaults(dir.path(), vec![]).unwrap();
    let repo = repository();
    remote(
        repo.path(),
        "origin",
        "git@git.example.com:team/project.git",
    );
    let query = GitPlatformQuery {
        repo: repo.path().to_string_lossy().into_owned(),
        provider: None,
        remote: None,
    };
    assert_eq!(
        crate::pull_requests::call(&manager, &query, "pull_requests.list", json!({}), &paths())
            .await
            .unwrap()["provider"],
        "custom.mjs"
    );
}

#[tokio::test]
async fn optional_metadata_respects_remote_selection_capabilities_and_configuration_changes() {
    let manager = GitPlatformProviderManager::default();
    manager
        .save(Settings {
            revision: 0,
            providers: vec![provider(FIXTURE)],
        })
        .unwrap();
    let repo = repository();
    let root = repo.path().to_str().unwrap();
    assert!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .is_none()
    );
    remote(
        repo.path(),
        "upstream",
        "git@git.example.com:team/project.git",
    );
    assert_eq!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .as_deref(),
        Some("https://avatars.example.com/team")
    );
    remote(
        repo.path(),
        "other",
        "git@git.example.com:other/project.git",
    );
    assert!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .is_none()
    );
    remote(
        repo.path(),
        "origin",
        "git@unsupported.example.com:owner/project.git",
    );
    assert!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        manager
            .commit_links(root, "origin", &"a".repeat(40), &paths())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "remote",
                "set-url",
                "origin",
                "git@git.example.com:owner/project.git"
            ])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .as_deref(),
        Some("https://avatars.example.com/owner")
    );
    let mut settings = manager.settings().unwrap();
    settings.providers[0].script = FIXTURE
        .replace(", 'repository.info'", "")
        .replace(",'repository.info'", "")
        .replace(",'repository.commit_links'", "");
    manager.save(settings).unwrap();
    assert!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        manager
            .commit_links(root, "origin", &"a".repeat(40), &paths())
            .await
            .unwrap()
            .is_none()
    );
    let mut settings = manager.settings().unwrap();
    settings.providers[0].script = FIXTURE.replace("https://avatars.example.com/", "javascript:");
    manager.save(settings).unwrap();
    assert!(manager.repository_avatar(root, &paths()).await.is_err());
    let mut settings = manager.settings().unwrap();
    settings.providers[0].script = FIXTURE.into();
    manager.save(settings).unwrap();
    assert_eq!(
        manager
            .repository_avatar(root, &paths())
            .await
            .unwrap()
            .as_deref(),
        Some("https://avatars.example.com/owner")
    );
}

#[test]
fn discovers_unregistered_files_and_refreshes_metadata_from_the_active_directory() {
    let state = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigStore::initialize(state.path()).unwrap();
    let script = FIXTURE.replace("name:'Custom'", "name:'My Forge'");
    config
        .save(
            Path::new("git-platform-providers/forge.v2.mjs"),
            script.as_bytes(),
        )
        .unwrap();
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![]).unwrap();
    let settings = manager.settings().unwrap();
    assert_eq!(settings.providers[0].id, "forge.v2.mjs");
    assert_eq!(settings.providers[0].name, "My Forge");
    let changed = script
        .replace("My Forge", "New Forge")
        .replace("git.example.com", "new.example.com");
    std::fs::write(
        config
            .directory()
            .join("git-platform-providers/forge.v2.mjs"),
        changed,
    )
    .unwrap();
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![]).unwrap();
    let settings = manager.settings().unwrap();
    assert_eq!(settings.providers[0].name, "New Forge");
    assert_eq!(settings.providers[0].hosts, ["new.example.com"]);
    let again = GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![]).unwrap();
    assert_eq!(again.settings().unwrap().revision, settings.revision);
    let mut removed = settings;
    removed.providers.clear();
    manager.save(removed).unwrap();
    assert!(
        !config
            .directory()
            .join("git-platform-providers/forge.v2.mjs")
            .exists()
    );
    assert!(
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![])
            .unwrap()
            .settings()
            .unwrap()
            .providers
            .is_empty()
    );
}

#[test]
fn discovery_skips_invalid_files_and_loads_repaired_scripts() {
    let state = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigStore::initialize(state.path()).unwrap();
    config
        .save(
            Path::new("git-platform-providers/custom.mjs"),
            FIXTURE.as_bytes(),
        )
        .unwrap();
    GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![]).unwrap();
    let directory = config.directory().join("git-platform-providers");
    std::fs::write(directory.join("custom.mjs"), "console.log('invalid')").unwrap();
    std::fs::write(directory.join("another.mjs"), FIXTURE).unwrap();
    std::fs::write(
        directory.join("write-api.mjs"),
        FIXTURE.replace("'repository.info']", "'merge']"),
    )
    .unwrap();
    std::fs::write(directory.join("bad name.mjs"), FIXTURE).unwrap();
    std::fs::write(directory.join("python.py"), FIXTURE).unwrap();
    std::fs::write(directory.join("unsupported.js"), FIXTURE).unwrap();
    std::fs::write(
        directory.join("oversized.mjs"),
        vec![b' '; SCRIPT_LIMIT + 1],
    )
    .unwrap();
    std::os::unix::fs::symlink(directory.join("another.mjs"), directory.join("link.mjs")).unwrap();
    std::fs::create_dir(directory.join("nested.mjs")).unwrap();
    std::fs::write(directory.join("nested.mjs/hidden.mjs"), FIXTURE).unwrap();
    let loaded = GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![])
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(
        loaded
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["another.mjs"]
    );
    std::fs::write(
        directory.join("custom.mjs"),
        FIXTURE.replace("git.example.com", "repaired.example.com"),
    )
    .unwrap();
    let loaded = GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![])
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(loaded.providers.len(), 2);
    assert_eq!(
        loaded
            .providers
            .iter()
            .find(|p| p.id == "custom.mjs")
            .unwrap()
            .hosts,
        ["repaired.example.com"]
    );
}

#[test]
fn discovery_is_isolated_by_home_and_preserves_existing_domain_ownership() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigStore::initialize(first.path()).unwrap();
    let other = aow_config::ConfigStore::initialize(second.path()).unwrap();
    config
        .save(
            Path::new("git-platform-providers/first.mjs"),
            FIXTURE.as_bytes(),
        )
        .unwrap();
    other
        .save(
            Path::new("git-platform-providers/second.mjs"),
            FIXTURE.as_bytes(),
        )
        .unwrap();
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(first.path(), vec![]).unwrap();
    assert_eq!(manager.settings().unwrap().providers[0].id, "first.mjs");
    config
        .save(
            Path::new("git-platform-providers/aaa.mjs"),
            FIXTURE.as_bytes(),
        )
        .unwrap();
    let loaded = GitPlatformProviderManager::persistent_with_defaults(first.path(), vec![])
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(loaded.providers.len(), 1);
    assert_eq!(loaded.providers[0].id, "first.mjs");
}

#[test]
fn discovery_ignores_files_outside_the_managed_directory() {
    let state = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigStore::initialize(state.path()).unwrap();
    let old_directory = config.directory().join("review-providers");
    std::fs::create_dir(&old_directory).unwrap();
    std::fs::write(old_directory.join("custom.mjs"), FIXTURE).unwrap();
    config
        .save(
            Path::new("git-platform-providers/current.mjs"),
            FIXTURE.as_bytes(),
        )
        .unwrap();
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![]).unwrap();
    let settings = manager.settings().unwrap();
    assert_eq!(settings.providers.len(), 1);
    assert_eq!(settings.providers[0].id, "current.mjs");
    assert!(
        !config
            .directory()
            .join("git-platform-providers/custom.mjs")
            .exists()
    );
    assert_eq!(
        std::fs::read_to_string(old_directory.join("custom.mjs")).unwrap(),
        FIXTURE
    );
}

#[test]
fn installed_defaults_never_replace_unregistered_local_files_or_restore_deleted_files() {
    let state = tempfile::tempdir().unwrap();
    let config = aow_config::ConfigStore::initialize(state.path()).unwrap();
    let custom = format!("{FIXTURE}\n// edited directly");
    config
        .save(
            Path::new("git-platform-providers/custom.mjs"),
            custom.as_bytes(),
        )
        .unwrap();
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![provider(FIXTURE)])
            .unwrap();
    assert_eq!(manager.settings().unwrap().providers[0].script, custom);
    std::fs::remove_file(config.directory().join("git-platform-providers/custom.mjs")).unwrap();
    let next =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![provider(FIXTURE)])
            .unwrap();
    assert!(next.settings().unwrap().providers.is_empty());
}

#[test]
fn release_updates_preserve_the_case_of_a_manually_renamed_provider_file() {
    let state = tempfile::tempdir().unwrap();
    let manager =
        GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![provider(FIXTURE)])
            .unwrap();
    let directory = manager
        .config
        .as_ref()
        .unwrap()
        .directory()
        .join("git-platform-providers");
    std::fs::rename(directory.join("custom.mjs"), directory.join("Custom.mjs")).unwrap();
    let updated = provider(&format!("{FIXTURE}\n// release update"));
    let loaded = GitPlatformProviderManager::persistent_with_defaults(state.path(), vec![updated])
        .unwrap()
        .settings()
        .unwrap();
    assert_eq!(loaded.providers.len(), 1);
    assert_eq!(loaded.providers[0].id, "Custom.mjs");
    assert_eq!(loaded.providers[0].script, FIXTURE);
}
