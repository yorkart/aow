import { basename, dirname, join } from 'node:path';
import { lstat } from 'node:fs/promises';
import { HttpError } from '../errors.js';
import { git, gitInfo } from './git.js';
import type { Tasks } from './tasks.js';

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
      if (!info.head || !/^[0-9a-f]{40,64}$/.test(info.head)) throw new HttpError(422, 'workspace_unavailable', '仓库尚无有效提交，不能创建独立 worktree。');
      const branch = `orbit/${org.slice(0, 8)}/${task.identifier}`;
      const directory = join(dirname(info.root), `${basename(info.root)}-orbit-${org.slice(0, 8)}-${task.identifier}`);
      if (await lstat(directory).then(() => true, () => false)) throw new HttpError(409, 'workspace_exists', '预期的任务工作区目录已存在，请先明确绑定已有工作区。');
      const result = await git(info.root, ['worktree', 'add', '-b', branch, directory, info.head], 30_000);
      if (!result.ok) throw new HttpError(409, 'worktree_creation_failed', '无法创建独立 worktree，请检查分支、目录和 Git 状态；现有文件未删除。');
      workspace = this.tasks.workspaces.remember(org, task.project_id, directory, 'execution', task.identifier, branch, info.head);
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
