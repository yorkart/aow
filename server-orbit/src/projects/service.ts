import { randomUUID } from 'node:crypto';
import { readdir } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { HttpError } from '../errors.js';
import { gitInfo, localDirectory } from './git.js';

export interface ProjectRow { id: string; org_id: string; name: string; directory: string | null; description: string; created_at: string; updated_at: string; revision: number }
export function shortText(value: unknown, label: string, limit: number, required = false): string {
  if (typeof value !== 'string' || value.length > limit || /\p{Cc}/u.test(value.replace(/[\n\r\t]/g, '')) || required && !value.trim()) throw new HttpError(422, 'invalid_project', `请填写有效的${label}。`);
  return value.trim();
}
export class Projects {
  constructor(readonly database: DatabaseSync) {}
  checkOrg(org: string) { if (!this.database.prepare('SELECT id FROM orgs WHERE id = ?').get(org)) throw new HttpError(404, 'org_not_found', '组织不存在。'); }
  row(org: string, id: string): ProjectRow {
    this.checkOrg(org);
    const row = this.database.prepare('SELECT * FROM projects WHERE org_id = ? AND id = ?').get(org, id) as unknown as ProjectRow | undefined;
    if (!row) throw new HttpError(404, 'project_not_found', '项目不存在。');
    return row;
  }
  public(row: ProjectRow) {
    const count = this.database.prepare('SELECT count(*) AS count FROM tasks WHERE org_id = ? AND project_id = ?').get(row.org_id, row.id) as { count: number };
    return { ...row, codebase_path: row.directory, task_count: count.count };
  }
  get(org: string, id: string) { return this.public(this.row(org, id)); }
  list(org: string) { this.checkOrg(org); return (this.database.prepare('SELECT * FROM projects WHERE org_id = ? ORDER BY name COLLATE NOCASE, id').all(org) as unknown as ProjectRow[]).map(row => this.public(row)); }
  async inspect(path: unknown) { const directory = await localDirectory(path); return { directory, ...(await gitInfo(directory)) }; }
  async directories(path?: string | null) {
    const directory = await localDirectory(path || homedir()); const entries = await readdir(directory, { withFileTypes: true });
    const children = entries.filter(entry => entry.isDirectory() && entry.name !== '.git').sort((a, b) => a.name.localeCompare(b.name));
    return { directory, parent: dirname(directory) === directory ? null : dirname(directory), directories: children.slice(0, 500).map(entry => ({ name: entry.name, path: join(directory, entry.name) })), truncated: children.length > 500 };
  }
  async save(org: string, input: Record<string, unknown>, id?: string) {
    this.checkOrg(org); const previous = id ? this.row(org, id) : undefined;
    if (previous && input.revision !== previous.revision) throw new HttpError(409, 'project_conflict', '项目已被其他窗口更新，请重新打开配置。');
    const name = shortText(input.name, '项目名称', 100, true); const description = shortText(input.description ?? '', '描述', 8000);
    const path = Object.hasOwn(input, 'codebase_path') ? input.codebase_path : input.directory;
    const inspection = path === undefined || path === null || path === '' ? null : await this.inspect(path);
    const directory = inspection?.directory ?? null; const now = new Date().toISOString(); const projectId = previous?.id ?? randomUUID();
    try {
      if (previous) {
        const result = this.database.prepare('UPDATE projects SET name = ?, description = ?, directory = ?, revision = revision + 1, updated_at = ? WHERE org_id = ? AND id = ? AND revision = ?').run(name, description, directory, now, org, projectId, previous.revision);
        if (!result.changes) throw new HttpError(409, 'project_conflict', '项目已被其他窗口更新，请重新打开配置。');
      } else this.database.prepare('INSERT INTO projects (id, org_id, name, directory, description, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)').run(projectId, org, name, directory, description, now, now);
    } catch (error) {
      if (error instanceof Error && error.message.includes('UNIQUE constraint failed: projects.org_id, projects.directory')) throw new HttpError(409, 'codebase_in_use', '当前组织已有项目使用此 Codebase，请选择该项目或其他目录。');
      throw error;
    }
    return { ...this.get(org, projectId), git: inspection ? { is_git: inspection.is_git, branch: inspection.branch, head: inspection.head } : null };
  }
  /** Backward-compatible registration used by Skills imports; paths remain deduplicated. */
  async register(org: string, input: Record<string, unknown>) {
    this.checkOrg(org); const directory = await localDirectory(input.directory);
    const existing = this.database.prepare('SELECT id FROM projects WHERE org_id = ? AND directory = ?').get(org, directory) as { id: string } | undefined;
    return existing ? this.get(org, existing.id) : this.save(org, { ...input, codebase_path: directory });
  }
}
