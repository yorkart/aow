use anyhow::Result;
use aow_zed::{AcpHost, AcpService};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

struct Host {
    settings: String,
    path: PathBuf,
}
#[async_trait::async_trait]
impl AcpHost for Host {
    async fn settings(&self) -> Result<String> {
        Ok(self.settings.clone())
    }
    async fn execution_path(&self) -> Result<Option<std::ffi::OsString>> {
        Ok(Some(self.path.clone().into_os_string()))
    }
    async fn read_text_file(&self, _cwd: &Path, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }
    async fn write_text_file(&self, _cwd: &Path, path: &Path, content: &str) -> Result<()> {
        Ok(std::fs::write(path, content)?)
    }
}

#[tokio::test]
async fn registry_install_is_independent_of_sessions_persistent_and_retryable() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let runtime = root.join("runtime");
    let cache = root.join("state");
    std::fs::create_dir_all(&runtime)?;
    std::fs::create_dir_all(&cache)?;
    let output = tokio::process::Command::new("node")
        .args(["-p", "process.execPath"])
        .output()
        .await?;
    assert!(output.status.success());
    let node = String::from_utf8(output.stdout)?.trim().to_owned();
    let started = root.join("adapter-started");
    let failed = root.join("fail-install");
    let calls = root.join("install-calls");
    let runtime_script = format!(
        "#!{node}\nif (process.argv[2] === '--version') console.log('v26.0.0'); else require('node:fs').writeFileSync({}, 'started');\n",
        json!(started)
    );
    let npm_script = format!(
        r#"#!{node}
const fs = require('node:fs');
fs.appendFileSync({calls}, 'install\n');
if (fs.existsSync({failed})) {{ console.error('fixture installation failed'); process.exit(1); }}
fs.mkdirSync('node_modules/@aow/fixture', {{ recursive: true }});
fs.writeFileSync('node_modules/@aow/fixture/package.json', '{{"name":"@aow/fixture","bin":"agent.js"}}');
fs.writeFileSync('node_modules/@aow/fixture/agent.js', 'console.log("adapter")');
"#,
        calls = json!(calls),
        failed = json!(failed)
    );
    for (name, code) in [("node", runtime_script), ("npm", npm_script)] {
        let path = runtime.join(name);
        std::fs::write(&path, code)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    std::fs::write(
        cache.join("registry.json"),
        serde_json::to_vec(&json!({"agents":[{
            "id":"fixture", "name":"Fixture", "version":"1.0.0", "description":"Installation fixture",
            "distribution":{"npx":{"package":"@aow/fixture@1.0.0"}}
        }]}))?,
    )?;
    let host = Arc::new(Host {
        settings: json!({"agent_servers":{"fixture":{"type":"registry","env":{}}}}).to_string(),
        path: runtime,
    });
    let service = AcpService::new(host.clone(), Some(cache.clone()))?;
    assert!(!service.agents().await?[0].installed);
    assert!(!service.registry(false).await?[0].installed);
    service.install_agent("fixture").await?;
    assert!(service.agents().await?[0].installed);
    assert!(service.registry(false).await?[0].installed);
    assert!(service.sessions(None).is_empty());
    assert!(
        !started.exists(),
        "Installation must not start an ACP adapter"
    );
    let status = service.installation_status("fixture").unwrap();
    assert_eq!(status.phase, "ready");
    assert!(!status.running);
    drop(service);
    let service = AcpService::new(host, Some(cache.clone()))?;
    assert!(
        service.agents().await?[0].installed,
        "Installed state must survive service restart"
    );
    service.install_agent("fixture").await?;
    assert_eq!(
        std::fs::read_to_string(&calls)?.lines().count(),
        1,
        "Reuse the installed package"
    );
    let version = std::fs::read_dir(cache.join("agents/fixture/npm"))?
        .next()
        .unwrap()?
        .path();
    std::fs::remove_file(version.join("node_modules/@aow/fixture/agent.js"))?;
    assert!(!service.agents().await?[0].installed);
    std::fs::write(&failed, "fail")?;
    assert!(service.install_agent("fixture").await.is_err());
    assert_eq!(
        service.installation_status("fixture").unwrap().phase,
        "failed"
    );
    assert!(!service.agents().await?[0].installed);
    std::fs::remove_file(failed)?;
    service.install_agent("fixture").await?;
    assert!(service.agents().await?[0].installed);
    assert!(service.sessions(None).is_empty());
    assert!(!started.exists());
    Ok(())
}
