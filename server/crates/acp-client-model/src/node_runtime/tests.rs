use super::*;

async fn real_node() -> Result<PathBuf> {
    let output = tokio::process::Command::new("node")
        .args(["--print", "process.execPath"])
        .output()
        .await?;
    ensure!(
        output.status.success(),
        "Node.js is required for npm integration tests"
    );
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

#[tokio::test]
async fn node_only_launcher_path_installs_and_runs_a_package_outside_the_workspace() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let launcher = temporary.path().join("launcher with spaces");
    std::fs::create_dir(&launcher)?;
    std::os::unix::fs::symlink(real_node().await?, launcher.join("node"))?;
    let environment = BTreeMap::from([
        ("npm_config_offline".into(), "true".into()),
        ("npm_config_userconfig".into(), "/dev/null".into()),
        (
            "npm_config_globalconfig".into(),
            temporary
                .path()
                .join("empty-npmrc")
                .to_string_lossy()
                .into_owned(),
        ),
    ]);
    let runtime = NodeRuntime::discover(Some(launcher.as_os_str()), &environment).await?;
    assert!(find_executable("npx", launcher.as_os_str()).is_none());
    assert!(find_executable("npm", launcher.as_os_str()).is_none());
    assert!(runtime.npm.is_absolute());
    assert_eq!(runtime.node, std::fs::canonicalize(real_node().await?)?);

    let archive = temporary.path().join("fixture.tgz");
    let writer = flate2::write::GzEncoder::new(
        std::fs::File::create(&archive)?,
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(writer);
    for (path, contents) in [
        (
            "package/package.json",
            r#"{"name":"@aow/fixture-agent","version":"1.0.0","bin":{"fixture-agent":"main.js"}}"#,
        ),
        (
            "package/main.js",
            "process.stdout.write(JSON.stringify({cwd:process.cwd(),argv:process.argv.slice(2)}));",
        ),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, contents.as_bytes())?;
    }
    tar.into_inner()?.finish()?;
    let cache = temporary.path().join("cache with spaces");
    let executable = runtime
        .install_package(
            &cache,
            "@aow/fixture-agent",
            archive.to_str().unwrap(),
            &environment,
            &Progress::default(),
        )
        .await?;
    assert!(executable.is_absolute());
    let workspace = temporary.path().join("workspace");
    std::fs::create_dir(&workspace)?;
    let output = tokio::process::Command::new(&runtime.node)
        .arg(&executable)
        .args(["literal argument", "$not_a_shell"])
        .current_dir(&workspace)
        .env("PATH", &runtime.execution_path)
        .output()
        .await?;
    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(
        result["cwd"],
        workspace.canonicalize()?.to_string_lossy().as_ref()
    );
    assert_eq!(
        result["argv"],
        serde_json::json!(["literal argument", "$not_a_shell"])
    );
    assert!(!workspace.join("package.json").exists());
    // A successful install can be reused offline after the source archive disappears.
    std::fs::remove_file(&archive)?;
    assert_eq!(
        runtime
            .install_package(
                &cache,
                "@aow/fixture-agent",
                archive.to_str().unwrap(),
                &environment,
                &Progress::default(),
            )
            .await?,
        executable
    );
    Ok(())
}

#[tokio::test]
async fn missing_runtime_has_an_actionable_error_without_using_the_parent_path() -> Result<()> {
    let empty = tempfile::tempdir()?;
    let error = NodeRuntime::discover(Some(empty.path().as_os_str()), &BTreeMap::new())
        .await
        .err()
        .context("Expected missing Node.js error")?;
    assert!(error.to_string().contains("Node.js 22"));
    assert!(error.to_string().contains("Settings → Environment"));
    Ok(())
}

#[tokio::test]
async fn npm_network_retries_are_visible_before_the_installer_exits() -> Result<()> {
    let mut child = tokio::process::Command::new(real_node().await?)
        .args(["-e", "process.stderr.write('npm http fetch attempt 1 failed with ETIMEDOUT\\n'); process.stdin.resume();"])
        .stdin(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true).spawn()?;
    let stderr = child.stderr.take().unwrap();
    let progress = Progress::default();
    progress.set("installing");
    let reading_progress = progress.clone();
    let reader = tokio::spawn(async move { read_install_output(stderr, &reading_progress).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while progress.snapshot().detail.as_deref() != Some("ETIMEDOUT") {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert!(child.try_wait()?.is_none());
    drop(child.stdin.take());
    child.wait().await?;
    assert!(reader.await??.contains("ETIMEDOUT"));
    Ok(())
}

#[test]
fn registry_versions_keep_the_upstream_npm_release_age_ceiling() {
    for (input, name, expected) in [
        (
            "@scope/agent@2.1.1",
            "@scope/agent",
            "@scope/agent@0.0.0 - 2.1.1",
        ),
        ("agent@latest", "agent", "agent@latest"),
        ("@scope/agent", "@scope/agent", "@scope/agent"),
    ] {
        assert_eq!(bounded_npm_package_spec(input), (name, expected.into()));
        assert!(valid_package_name(name));
    }
    for name in [
        "../outside",
        "@scope/..",
        "-option",
        "/absolute",
        "scope/agent",
        "@/agent",
    ] {
        assert!(!valid_package_name(name));
    }
}

#[tokio::test]
async fn npm_bin_selection_rejects_ambiguous_and_escaping_entrypoints() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("agent");
    std::fs::create_dir(&package)?;
    std::fs::write(package.join("main.js"), "")?;
    for bin in [
        serde_json::json!("main.js"),
        serde_json::json!({"agent":"main.js","other":"other.js"}),
    ] {
        std::fs::write(
            package.join("package.json"),
            serde_json::to_vec(&serde_json::json!({"bin":bin}))?,
        )?;
        assert_eq!(
            read_package_executable(directory.path(), "agent").await?,
            package.join("main.js").canonicalize()?
        );
    }
    for bin in [
        serde_json::json!("../outside.js"),
        serde_json::json!({"first":"main.js","second":"main.js"}),
    ] {
        std::fs::write(directory.path().join("outside.js"), "")?;
        std::fs::write(
            package.join("package.json"),
            serde_json::to_vec(&serde_json::json!({"bin":bin}))?,
        )?;
        assert!(
            read_package_executable(directory.path(), "agent")
                .await
                .is_err()
        );
    }
    Ok(())
}
