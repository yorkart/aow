import { dirname } from 'node:path';
import { mkdir } from 'node:fs/promises';
import { HttpError } from '../errors.js';
import { git, gitInfo } from './git.js';
import type { Tasks } from './tasks.js';
import { taskWorktreePlan } from './worktree-settings.js';

/** Invoked by a task runner, never by workspace refresh or project configuration. */
export class TaskWorkspaces {
  constructor(private tasks: Tasks) {}
  async prepare(org: string, taskId: string, isolated = true) {
    const task = this.tasks.get(org, taskId);
    if (task.workspace_id) return this.tasks.workspaces.resolve(org, task.project_id, task.workspace_id);
    const main = await this.tasks.workspaces.resolve(org, task.project_id);
    const info = await gitInfo(main.directory);
    let workspace = main;
    if (isolated && info.is_git && info.root) {
      const { branch, directory, baseCommit } = await taskWorktreePlan(info.root, org, task, task.project.worktree_settings);
      await mkdir(dirname(directory), { recursive: true, mode: 0o700 });
      const result = await git(info.root, ['worktree', 'add', '-b', branch, directory, baseCommit], 30_000);
      if (!result.ok) throw new HttpError(409, 'worktree_creation_failed', '无法创建独立 worktree，请检查分支、目录和 Git 状态；现有文件未删除。');
      workspace = this.tasks.workspaces.remember(org, task.project_id, directory, 'execution', task.identifier, branch, baseCommit);
    }
    // A concurrent binding wins. Newly created worktrees remain visible and are never deleted.
    const result = this.tasks.projects.database.prepare('UPDATE tasks SET workspace_id = ?, revision = revision + 1, updated_at = ? WHERE org_id = ? AND id = ? AND workspace_id IS NULL AND revision = ?').run(workspace.id, new Date().toISOString(), org, taskId, task.revision);
    if (!result.changes) {
      const current = this.tasks.row(org, taskId);
      if (current.workspace_id) return this.tasks.workspaces.resolve(org, current.project_id, current.workspace_id);
      throw new HttpError(409, 'task_conflict', '任务已改变，请重新准备工作区。');
    }
    return workspace;
  }
}
