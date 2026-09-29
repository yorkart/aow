use std::path::{Path, PathBuf};

use super::super::*;

impl AowManager {
    pub(crate) async fn automation_project(
        &self,
        id: &str,
        worktree_path: &std::path::Path,
    ) -> Result<(String, PathBuf), AowError> {
        let project = self.project(id).await?;
        let requested = paths::canonical_directory(worktree_path).await?;
        if project.error.is_some()
            || !project
                .worktrees
                .iter()
                .any(|worktree| Path::new(&worktree.path) == requested)
        {
            return Err(AowError::Invalid("工作区不属于所选项目".into()));
        }
        Ok((project.name, PathBuf::from(project.registered_path)))
    }

    pub(crate) async fn resolve_agent_launch(
        &self,
        id: &str,
        worktree_path: &str,
    ) -> Result<AgentLaunch, AowError> {
        let requested = paths::canonical_directory(Path::new(worktree_path)).await?;
        let projects = self.projects().await?;
        let registered = projects.iter().any(|project| {
            project.error.is_none()
                && project
                    .worktrees
                    .iter()
                    .any(|worktree| Path::new(&worktree.path) == requested)
        });
        if !registered {
            return Err(AowError::Invalid(format!(
                "agent cwd is not a registered worktree: {}",
                requested.display()
            )));
        }
        let path = self.execution_path().await?;
        let agent = self
            .agents_in_path(&path)?
            .into_iter()
            .find(|agent| agent.id == id && agent.available)
            .ok_or_else(|| AowError::AgentNotFound(id.to_owned()))?;
        let mut launch = agent.into_launch(&path)?;
        if let Some(state_dir) = &self.inner.state_dir {
            // Child tools must reach this AoW instance even when terminald or
            // the agent profile has a different default state directory.
            launch.env.insert(
                "AOW_STATE_DIR".into(),
                state_dir.to_string_lossy().into_owned(),
            );
        }
        Ok(launch)
    }

    pub(crate) async fn resolve_terminal_rebuild_launch(
        &self,
        pane: &aow_protocol::TerminalPane,
        workspace_root: &str,
    ) -> Result<AgentLaunch, AowError> {
        let profile_id =
            if let Some(id) = &pane.agent_profile_id {
                id.clone()
            } else {
                // Older metadata only saved the product type. Recover the profile
                // only when its executable and full launch arguments identify it.
                let candidates: Vec<_> = self.agents().await?.into_iter().filter(|agent| {
                let Some(kind) = agent.agent_type else {
                    return false;
                };
                if !agent.available
                    || Some(kind.id()) != pane.agent_id.as_deref()
                    || agent.executable.as_deref() != Some(pane.shell.as_str())
                {
                    return false;
                }
                if agent.args == pane.arguments {
                    return true;
                }
                let suffix = pane.arguments.strip_prefix(agent.args.as_slice());
                let resume = if kind.id() == "claude" { "--resume" } else { "resume" };
                matches!(suffix, Some([flag, session]) if flag == resume && !session.is_empty())
            }).collect();
                if candidates.len() != 1 {
                    return Err(AowError::Invalid(
                        "无法唯一确定该终端原来的 Agent 配置，请检查 Agent 配置后重试".into(),
                    ));
                }
                candidates[0].id.clone()
            };
        let launch = self
            .resolve_agent_launch(&profile_id, workspace_root)
            .await?;
        if Some(launch.agent_type.id()) != pane.agent_id.as_deref() {
            return Err(AowError::Invalid(
                "原 Agent 配置的类型已改变，无法重建".into(),
            ));
        }
        Ok(launch)
    }

    pub(crate) async fn agent_worktree_path(
        &self,
        project_id: &str,
        worktree_path: &str,
    ) -> Result<String, AowError> {
        let project = self.project(project_id).await?;
        let requested = paths::canonical_directory(Path::new(worktree_path)).await?;
        if !project
            .worktrees
            .iter()
            .any(|worktree| Path::new(&worktree.path) == requested)
        {
            return Err(AowError::Invalid(format!(
                "agent cwd is not a worktree of project {project_id}: {}",
                requested.display()
            )));
        }
        Ok(requested.to_string_lossy().into_owned())
    }
}
