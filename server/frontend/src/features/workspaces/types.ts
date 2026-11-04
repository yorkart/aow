import { t } from '../../lib/i18n';
export type WorkspaceMode = 'new_worktree' | 'existing' | 'temporary' | 'dynamic';

export interface WorkspaceConfig {
  workspace_mode: WorkspaceMode;
  workspace_path: string;
  base_branch: string;
}

export const workspaceNames: Record<WorkspaceMode, string> = {
  new_worktree: t("workspaces.new_worktree"), existing: t("workspaces.existing_worktree"), temporary: t("workspaces.temporary_workspace"), dynamic: t("workspaces.choose_at_runtime"),
};

/** Send only the fields used by the selected mode. */
export function workspaceConfig(value: WorkspaceConfig): WorkspaceConfig {
  return {
    workspace_mode: value.workspace_mode,
    workspace_path: value.workspace_mode === 'existing' ? value.workspace_path : '',
    base_branch: value.workspace_mode === 'new_worktree' ? value.base_branch : '',
  };
}
