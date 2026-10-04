export type WorkspaceMode = 'new_worktree' | 'existing' | 'temporary' | 'dynamic';

export interface WorkspaceConfig {
  workspace_mode: WorkspaceMode;
  workspace_path: string;
  base_branch: string;
}

export const workspaceNames: Record<WorkspaceMode, string> = {
  new_worktree: '新建 Worktree', existing: '已有 Worktree', temporary: '临时工作区', dynamic: '动态指定',
};

/** Send only the fields used by the selected mode. */
export function workspaceConfig(value: WorkspaceConfig): WorkspaceConfig {
  return {
    workspace_mode: value.workspace_mode,
    workspace_path: value.workspace_mode === 'existing' ? value.workspace_path : '',
    base_branch: value.workspace_mode === 'new_worktree' ? value.base_branch : '',
  };
}
