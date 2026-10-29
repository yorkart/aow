import { randomUUID } from 'node:crypto';
import { basename } from 'node:path';
import { HttpError } from '../errors.js';
import { gitInfo, localDirectory, type GitWorkspace } from './git.js';
import type { Projects } from './service.js';

export interface WorkspaceRow { id: string; org_id: string; project_id: string; name: string; kind: 'project' | 'execution'; directory: string; task_branch: string | null; base_commit: string | null; last_used_at: string | null; created_at: string }
export class Workspaces {
  constructor(readonly projects: Projects) {}
  row(org: string, project: string, id: string) {
    this.projects.row(org, project);
    const row = this.projects.database.prepare('SELECT * FROM project_workspaces WHERE org_id = ? AND project_id = ? AND id = ?').get(org, project, id) as unknown as WorkspaceRow | undefined;
    if (!row) throw new HttpError(404, 'workspace_not_found', '当前项目中没有此工作区。');
    return row;
  }
  remember(org: string, project: string, directory: string, kind: 'project' | 'execution', name?: string, branch?: string | null, base?: string | null) {
    this.projects.row(org, project);
    this.projects.database.prepare(`INSERT INTO project_workspaces (id, org_id, project_id, name, kind, directory, task_branch, base_commit, created_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(org_id, project_id, directory) DO NOTHING`).run(randomUUID(), org, project, name ?? basename(directory), kind, directory, branch ?? null, base ?? null, new Date().toISOString());
    return this.projects.database.prepare('SELECT * FROM project_workspaces WHERE org_id = ? AND project_id = ? AND directory = ?').get(org, project, directory) as unknown as WorkspaceRow;
  }
  async available(row: WorkspaceRow) {
    const canonical = await localDirectory(row.directory).catch(() => null);
    return canonical === row.directory;
  }
  async list(org: string, projectId: string) {
    const project = this.projects.row(org, projectId); const discovered = new Map<string, GitWorkspace>(); const warnings: string[] = [];
    const directory = this.projects.directory(project);
    await this.projects.prepareDirectory(project).catch((error: unknown) => { warnings.push(error instanceof Error ? error.message : '无法准备项目工作目录。'); });
    this.remember(org, projectId, directory, 'project', project.name);
    if (await localDirectory(directory).catch(() => null) === directory) {
      const info = await gitInfo(directory); if (info.warning) warnings.push(info.warning);
      for (const workspace of info.worktrees) {
        const path = await localDirectory(workspace.directory).catch(() => workspace.directory);
        discovered.set(path, workspace); this.remember(org, projectId, path, path === directory ? 'project' : 'execution');
      }
      if (!discovered.has(directory)) discovered.set(directory, { directory, branch: info.branch, head: info.head, locked: false, prunable: false });
    }
    const rows = this.projects.database.prepare('SELECT * FROM project_workspaces WHERE org_id = ? AND project_id = ?').all(org, projectId) as unknown as WorkspaceRow[];
    const workspaces = await Promise.all(rows.map(async row => {
      const tasks = this.projects.database.prepare('SELECT id, identifier, title, status FROM tasks WHERE org_id = ? AND project_id = ? AND workspace_id = ? ORDER BY updated_at DESC').all(org, projectId, row.id) as { id: string; identifier: string; title: string; status: string }[];
      const primary = row.directory === directory;
      if (!primary && !discovered.has(row.directory) && !tasks.length && !row.last_used_at) return undefined;
      const available = await this.available(row); const known = discovered.get(row.directory);
      const info = available && !known ? await gitInfo(row.directory) : undefined;
      return { ...row, name: primary ? project.name : row.name, is_primary: primary, available, availability_message: available ? '目录可访问' : '目录丢失、已移动或无法访问', branch: known?.branch ?? info?.branch ?? null, head: known?.head ?? info?.head ?? null, locked: known?.locked ?? false, prunable: known?.prunable ?? false, tasks };
    }));
    return { project: this.projects.get(org, projectId), warnings, workspaces: workspaces.filter(item => item !== undefined).sort((a, b) => Number(b.is_primary) - Number(a.is_primary) || (b.last_used_at ?? '').localeCompare(a.last_used_at ?? '') || a.name.localeCompare(b.name) || a.id.localeCompare(b.id)) };
  }
  used(org: string, project: string, id: string) { this.row(org, project, id); this.projects.database.prepare('UPDATE project_workspaces SET last_used_at = ? WHERE org_id = ? AND project_id = ? AND id = ?').run(new Date().toISOString(), org, project, id); }
  async resolve(org: string, projectId: string, id?: string) {
    const project = this.projects.row(org, projectId);
    const row = id ? this.row(org, projectId, id) : this.remember(org, projectId, await this.projects.prepareDirectory(project), 'project', project.name);
    if (!await this.available(row)) throw new HttpError(422, 'workspace_unavailable', '绑定的工作区丢失、已移动或无法访问，请先修复工作区。');
    return row;
  }
}
