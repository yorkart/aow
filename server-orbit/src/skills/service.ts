import { randomUUID } from 'node:crypto';
import { mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { transaction } from '../database.js';
import { HttpError } from '../errors.js';
import { createEntry, metadata, renameEntry } from './metadata.js';
import { packageHash, readPackage, validatePackage, writePackage, type PackageEntry, type PackageFile } from './files.js';

export interface Skill {
  id: string; org_id: string; name: string; description: string; current_version_id: string;
  source_project_id: string | null; source_path: string | null; updated_at: string;
  version: number; content_hash: string; agent_count: number;
}
interface Version { id: string; skill_id: string; directory: string; number: number }
interface Prepared { id: string; versionId: string; directory: string; files: PackageEntry[]; name: string; description: string; hash: string }

export class Skills {
  constructor(readonly database: DatabaseSync, readonly stateDir: string) {}

  checkOrg(org: string) {
    if (!this.database.prepare('SELECT id FROM orgs WHERE id = ?').get(org)) throw new HttpError(404, 'org_not_found', '组织不存在。');
  }

  list(org: string, query = ''): Skill[] {
    this.checkOrg(org);
    return this.database.prepare(`SELECT s.*, v.number AS version, v.content_hash,
      (SELECT count(*) FROM agent_skills a WHERE a.org_id = s.org_id AND a.skill_id = s.id) AS agent_count
      FROM skills s JOIN skill_versions v ON v.id = s.current_version_id AND v.skill_id = s.id
      WHERE s.org_id = ? AND s.deleted_at IS NULL AND instr(lower(s.name || ' ' || s.description), lower(?)) > 0
      ORDER BY s.updated_at DESC, s.id`).all(org, query) as unknown as Skill[];
  }

  get(org: string, id: string): Skill {
    this.checkOrg(org);
    const skill = this.database.prepare(`SELECT s.*, v.number AS version, v.content_hash,
      (SELECT count(*) FROM agent_skills a WHERE a.org_id = s.org_id AND a.skill_id = s.id) AS agent_count
      FROM skills s JOIN skill_versions v ON v.id = s.current_version_id AND v.skill_id = s.id
      WHERE s.org_id = ? AND s.id = ? AND s.deleted_at IS NULL`).get(org, id) as unknown as Skill | undefined;
    if (!skill) throw new HttpError(404, 'skill_not_found', '技能不存在或已删除。');
    return skill;
  }

  async detail(org: string, id: string) {
    const skill = this.get(org, id);
    const version = this.version(skill.current_version_id);
    const files = await readPackage(version.directory);
    const agents = this.database.prepare('SELECT agent_id FROM agent_skills WHERE org_id = ? AND skill_id = ? ORDER BY agent_id').all(org, id) as unknown as { agent_id: string }[];
    return { ...skill, files, agents };
  }

  version(id: string): Version {
    const version = this.database.prepare('SELECT * FROM skill_versions WHERE id = ?').get(id) as unknown as Version | undefined;
    if (!version) throw new HttpError(404, 'version_not_found', '技能版本不存在。');
    return version;
  }

  private conflict(error: unknown): never {
    if (error instanceof Error && error.message.includes('UNIQUE constraint failed: skills.org_id, skills.name')) {
      throw new HttpError(409, 'skill_name_conflict', '组织中已存在同名技能，请修改名称后保存。');
    }
    throw error;
  }

  private async prepare(org: string, id: string, input: unknown): Promise<Prepared> {
    const files = validatePackage(input);
    const entry = files.find(file => file.path === 'SKILL.md') as PackageFile;
    const fields = metadata(entry.content);
    const versionId = randomUUID();
    const parent = join(this.stateDir, 'skills', org, id, 'versions');
    await mkdir(parent, { recursive: true, mode: 0o700 });
    const directory = join(parent, versionId);
    try { await writePackage(directory, files); }
    catch (error) { await rm(directory, { recursive: true, force: true }); throw error; }
    return { id, versionId, directory, files, ...fields, hash: packageHash(files) };
  }

  private insertVersion(prepared: Prepared, number: number, now: string) {
    this.database.prepare('INSERT INTO skill_versions VALUES (?, ?, ?, ?, ?, ?)')
      .run(prepared.versionId, prepared.id, number, prepared.directory, prepared.hash, now);
  }

  private insert(org: string, prepared: Prepared, project: string | null, source: string | null) {
    const now = new Date().toISOString();
    this.database.prepare(`INSERT INTO skills (id, org_id, name, description, current_version_id, source_project_id, source_path, updated_at)
      VALUES (?, ?, ?, ?, ?, ?, ?, ?)`).run(prepared.id, org, prepared.name, prepared.description, prepared.versionId, project, source, now);
    this.insertVersion(prepared, 1, now);
  }

  async create(org: string, request: Record<string, unknown>) {
    this.checkOrg(org);
    const content = createEntry(request.name, request.description, request.instructions);
    const prepared = await this.prepare(org, randomUUID(), [{ path: 'SKILL.md', kind: 'file', encoding: 'utf8', content, executable: false }]);
    try { transaction(this.database, () => this.insert(org, prepared, null, null)); }
    catch (error) { await rm(prepared.directory, { recursive: true, force: true }); this.conflict(error); }
    return this.detail(org, prepared.id);
  }

  async save(org: string, id: string, request: Record<string, unknown>) {
    const previous = this.get(org, id);
    if (request.base_version_id !== previous.current_version_id) throw new HttpError(409, 'version_conflict', '技能已被更新，请重新打开后编辑；当前草稿尚未保存。');
    const prepared = await this.prepare(org, id, request.files);
    try {
      transaction(this.database, () => {
        const now = new Date().toISOString();
        const updated = this.database.prepare(`UPDATE skills SET name = ?, description = ?, current_version_id = ?, updated_at = ?
          WHERE id = ? AND org_id = ? AND current_version_id = ? AND deleted_at IS NULL`)
          .run(prepared.name, prepared.description, prepared.versionId, now, id, org, previous.current_version_id);
        if (!updated.changes) throw new HttpError(409, 'version_conflict', '技能已被更新，请重新打开后编辑。');
        this.insertVersion(prepared, previous.version + 1, now);
      });
    } catch (error) { await rm(prepared.directory, { recursive: true, force: true }); this.conflict(error); }
    return this.detail(org, id);
  }

  remove(org: string, id: string) {
    transaction(this.database, () => {
      const skill = this.get(org, id);
      if (skill.agent_count) throw new HttpError(409, 'skill_in_use', '请先在 Agent 配置中解除此技能的引用，再删除。');
      this.database.prepare('UPDATE skills SET deleted_at = ? WHERE id = ? AND org_id = ?').run(new Date().toISOString(), id, org);
    });
  }

  async importPackages(org: string, projectId: string, packages: { path: string; name?: string; files: PackageEntry[] }[]) {
    this.checkOrg(org);
    const prepared: Prepared[] = [];
    try {
      for (const pack of packages) {
        const files = pack.files.map(file => file.path === 'SKILL.md' && file.kind === 'file' && pack.name !== undefined && pack.name.trim() !== metadata(file.content).name ? { ...file, content: renameEntry(file.content, pack.name) } : file);
        prepared.push(await this.prepare(org, randomUUID(), files));
      }
      transaction(this.database, () => prepared.forEach((item, i) => this.insert(org, item, projectId, packages[i].path)));
    } catch (error) {
      await Promise.all(prepared.map(item => rm(item.directory, { recursive: true, force: true })));
      this.conflict(error);
    }
    return prepared.map(item => this.get(org, item.id));
  }
}
