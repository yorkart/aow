use super::*;

impl AowManager {
    pub(in crate::aow) async fn project_branches(
        &self,
        id: &str,
    ) -> Result<ProjectBranches, AowError> {
        let project = self.project(id).await?;
        let output = git_output(
            Path::new(&project.registered_path),
            &[
                "for-each-ref",
                "--format=%(refname)%09%(symref)",
                "refs/heads/",
                "refs/remotes/",
            ],
        )
        .await?;
        let refs: Vec<_> = output
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .collect();
        let mut branches: Vec<_> = refs
            .iter()
            .filter(|(_, target)| target.is_empty())
            .filter_map(|(name, _)| {
                name.strip_prefix("refs/heads/")
                    .or_else(|| name.strip_prefix("refs/remotes/"))
                    .map(str::to_owned)
            })
            .collect();
        branches.sort();
        branches.dedup();
        let remote_default = refs.iter().find_map(|(name, target)| {
            (*name == "refs/remotes/origin/HEAD")
                .then(|| target.strip_prefix("refs/remotes/"))
                .flatten()
        });
        let main_branch = project
            .worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .map(|worktree| worktree.branch.as_str());
        let default_branch = remote_default
            .filter(|branch| branches.iter().any(|value| value == branch))
            .or_else(|| main_branch.filter(|branch| branches.iter().any(|value| value == branch)))
            .or_else(|| branches.first().map(String::as_str))
            .unwrap_or_default()
            .to_owned();
        Ok(ProjectBranches {
            branches,
            default_branch,
        })
    }
}
