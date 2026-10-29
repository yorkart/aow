import { randomUUID } from 'node:crypto';
import { transaction } from '../database.js';
import { HttpError } from '../errors.js';
import { shortText, type Projects } from './service.js';
import type { Workspaces } from './workspaces.js';

export const taskStatuses = ['todo', 'in_progress', 'blocked', 'done', 'cancelled'] as const;
export interface TaskRow { id: string; org_id: string; project_id: string; identifier: string; title: string; description: string; status: string; agent_id: string | null; workspace_id: string | null; revision: number; created_at: string; updated_at: string }
export class Tasks {
  constructor(readonly projects: Projects, readonly workspaces: Workspaces) {}
  row(org: string, id: string): TaskRow {
    this.projects.checkOrg(org);
    const row = this.projects.database.prepare('SELECT * FROM tasks WHERE org_id = ? AND id = ?').get(org, id) as unknown as TaskRow | undefined;
    if (!row) throw new HttpError(404, 'task_not_found', '任务不存在。');
    return row;
  }
  get(org: string, id: string) {
    const row = this.row(org, id);
    const agent = row.agent_id ? this.projects.database.prepare('SELECT id, name, enabled FROM agents WHERE org_id = ? AND id = ?').get(org, row.agent_id) as { id: string; name: string; enabled: number } : null;
    return { ...row, agent, project: this.projects.get(org, row.project_id), workspace: row.workspace_id ? this.workspaces.row(org, row.project_id, row.workspace_id) : null };
  }
  list(org: string, project: string, query = '', status = '') {
    this.projects.row(org, project);
    if (status && !taskStatuses.includes(status as typeof taskStatuses[number])) throw new HttpError(422, 'invalid_task', '任务状态无效。');
    return (this.projects.database.prepare(`SELECT id FROM tasks WHERE org_id = ? AND project_id = ? AND (? = '' OR status = ?) AND instr(lower(identifier || ' ' || title), lower(?)) > 0 ORDER BY updated_at DESC, identifier DESC`).all(org, project, status, status, query) as { id: string }[]).map(row => this.get(org, row.id));
  }
  private agent(org: string, id: unknown, previous?: string | null): string | null {
    if (id === undefined || id === null || id === '') return null;
    const row = typeof id === 'string' ? this.projects.database.prepare('SELECT enabled FROM agents WHERE org_id = ? AND id = ?').get(org, id) as { enabled: number } | undefined : undefined;
    if (!row || !row.enabled && id !== previous) throw new HttpError(422, 'invalid_task_agent', '请选择当前组织中已启用的 Agent。');
    return id as string;
  }
  async save(org: string, input: Record<string, unknown>, id?: string) {
    const previous = id ? this.row(org, id) : undefined; const project = previous?.project_id ?? input.project_id;
    if (previous && input.revision !== previous.revision) throw new HttpError(409, 'task_conflict', '任务已被更新，请重新打开详情。');
    if (previous && input.project_id !== undefined && input.project_id !== previous.project_id) throw new HttpError(422, 'invalid_task', '不能在此详情页将任务移动到其他项目。');
    if (typeof project !== 'string') throw new HttpError(422, 'invalid_task', '请选择项目。');
    this.projects.row(org, project);
    const title = shortText(input.title ?? previous?.title, '任务标题', 200, true); const description = shortText(input.description ?? previous?.description ?? '', '任务描述', 16000);
    const status = input.status ?? previous?.status ?? 'todo';
    if (!taskStatuses.includes(status as typeof taskStatuses[number])) throw new HttpError(422, 'invalid_task', '任务状态无效。');
    const agent = this.agent(org, input.agent_id === undefined ? previous?.agent_id : input.agent_id, previous?.agent_id);
    const workspaceId = input.workspace_id === undefined ? previous?.workspace_id ?? null : input.workspace_id;
    if (workspaceId !== null) {
      if (typeof workspaceId !== 'string') throw new HttpError(422, 'invalid_task', '工作区无效。');
      // Unchanged missing workspaces remain attached; unrelated task edits must preserve them.
      if (workspaceId !== previous?.workspace_id) await this.workspaces.resolve(org, project, workspaceId);
      else this.workspaces.row(org, project, workspaceId);
    }
    const taskId = previous?.id ?? randomUUID(); const now = new Date().toISOString();
    transaction(this.projects.database, () => {
      if (previous) {
        const result = this.projects.database.prepare('UPDATE tasks SET title = ?, description = ?, status = ?, agent_id = ?, workspace_id = ?, revision = revision + 1, updated_at = ? WHERE org_id = ? AND id = ? AND revision = ?').run(title, description, String(status), agent, workspaceId, now, org, taskId, previous.revision);
        if (!result.changes) throw new HttpError(409, 'task_conflict', '任务已被更新，请重新打开详情。');
      } else {
        this.projects.database.prepare('INSERT INTO task_counters VALUES (?, 1) ON CONFLICT(org_id) DO NOTHING').run(org);
        const counter = this.projects.database.prepare('SELECT next_number FROM task_counters WHERE org_id = ?').get(org) as { next_number: number };
        this.projects.database.prepare('UPDATE task_counters SET next_number = next_number + 1 WHERE org_id = ?').run(org);
        this.projects.database.prepare('INSERT INTO tasks (id, org_id, project_id, identifier, title, description, status, agent_id, workspace_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)').run(taskId, org, project, `ORB-${counter.next_number}`, title, description, String(status), agent, workspaceId, now, now);
      }
    });
    return this.get(org, taskId);
  }
}
