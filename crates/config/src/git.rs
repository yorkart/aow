use std::{
    path::Path,
    process::{Command, Output, Stdio},
};

use anyhow::{Context, Result, ensure};

pub(super) fn git_command(repository: &Path) -> Command {
    let mut command = Command::new("git");
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(name);
    }
    command
        .current_dir(repository)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LITERAL_PATHSPECS", "1")
        .args([
            "-c",
            "user.name=AoW",
            "-c",
            "user.email=aow@localhost",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .stdin(Stdio::null());
    command
}

pub(super) fn git_checked(command: &mut Command) -> Result<Output> {
    let output = command.output().context("无法执行配置仓库 Git 命令")?;
    ensure!(
        output.status.success(),
        "配置仓库 Git 操作失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output)
}

pub(super) fn git_text(repository: &Path, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(git_checked(git_command(repository).args(args))?.stdout)?
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    )
}
