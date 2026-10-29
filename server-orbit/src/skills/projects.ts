import { constants } from 'node:fs';
import { lstat, open, readdir, realpath } from 'node:fs/promises';
import { join, resolve, sep } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { HttpError } from '../errors.js';
import { bytes, packagePath, readPackage, MAX_FILE_BYTES, MAX_PACKAGE_BYTES } from './files.js';
import { metadata } from './metadata.js';
import type { Skills } from './service.js';
import { Projects } from '../projects/service.js';

interface Project { id: string; org_id: string; name: string; directory: string | null }
export class SkillProjects {
  constructor(private skills: Skills, private database: DatabaseSync = skills.database) {}

  list(org: string): Project[] {
    this.skills.checkOrg(org);
    return new Projects(this.database).list(org);
  }

  async create(org: string, input: Record<string, unknown>): Promise<Project> {
    this.skills.checkOrg(org);
    return new Projects(this.database).register(org, input);
  }

  private async get(org: string, id: unknown): Promise<Project & { directory: string }> {
    this.skills.checkOrg(org);
    if (typeof id !== 'string') throw new HttpError(422, 'invalid_project', '请选择当前组织的项目。');
    const project = this.database.prepare('SELECT * FROM projects WHERE id = ? AND org_id = ?').get(id, org) as unknown as Project | undefined;
    if (!project) throw new HttpError(404, 'project_not_found', '当前组织中没有这个项目。');
    if (!project.directory) throw new HttpError(422, 'workspace_required', '项目尚未配置 Codebase，请先在 Projects 的 Configuration 中配置本地目录。');
    const canonical = await realpath(project.directory).catch(() => undefined);
    if (canonical !== project.directory) throw new HttpError(422, 'project_changed', '项目工作目录已移动或变更，请重新登记。');
    return { ...project, directory: project.directory };
  }

  async scan(org: string, projectId: unknown) {
    const project = await this.get(org, projectId);
    const found: { path: string; name: string; description: string }[] = [];
    const warnings: string[] = [];
    const ignored = new Set(['.git', 'node_modules', 'target', 'dist', 'build', '.venv', 'venv']);
    let visited = 0;
    let exhausted = false;
    const walk = async (relative: string, depth: number) => {
      if (exhausted) return;
      if (++visited > 10_000 || depth > 16 || found.length >= 100) { warnings.push('扫描达到数量或目录深度限制，请选择更具体的项目目录。'); exhausted = true; return; }
      const directory = join(project.directory, relative);
      try {
        if ((await lstat(directory)).isSymbolicLink() || await realpath(directory) !== directory) throw new Error('扫描跳过符号链接目录。');
        const children = await readdir(directory, { withFileTypes: true });
        const entry = children.find(item => item.name === 'SKILL.md');
        if (entry) {
          try {
            if (!entry.isFile()) throw new Error('SKILL.md 必须是普通文件。');
            const file = join(directory, 'SKILL.md');
            const handle = await open(file, constants.O_RDONLY | constants.O_NOFOLLOW);
            try {
              if ((await handle.stat()).size > MAX_FILE_BYTES) throw new Error('SKILL.md 超过 2 MiB。');
              const content = await handle.readFile();
              if (content.length > MAX_FILE_BYTES) throw new Error('SKILL.md 超过 2 MiB。');
              const fields = metadata(content.toString('utf8'));
              found.push({ path: relative || '.', ...fields });
            } finally { await handle.close(); }
          } catch (error) { warnings.push(`${relative || '.'}/SKILL.md：${error instanceof Error ? error.message : '无法读取'}`); }
        }
        for (const child of children) {
          if (child.isDirectory() && !ignored.has(child.name)) await walk(relative ? `${relative}/${child.name}` : child.name, depth + 1);
        }
      } catch (error) { warnings.push(`${relative || '.'}：${error instanceof Error ? error.message : '无法读取'}`); }
    };
    await walk('', 0);
    return { project, skills: found, warnings };
  }

  async import(org: string, input: Record<string, unknown>) {
    const project = await this.get(org, input.project_id);
    if (!Array.isArray(input.skills) || !input.skills.length || input.skills.length > 100) throw new HttpError(422, 'invalid_import', '请选择 1–100 个技能。');
    const packages = [];
    const paths = new Set<string>();
    let total = 0;
    for (const item of input.skills as unknown[]) {
      if (!item || typeof item !== 'object') throw new HttpError(422, 'invalid_import', '导入条目无效。');
      const value = item as Record<string, unknown>;
      const path = value.path === '.' ? '.' : packagePath(value.path);
      if (paths.has(path)) throw new HttpError(422, 'invalid_import', '不能重复导入同一个路径。');
      paths.add(path);
      if (value.name !== undefined && typeof value.name !== 'string') throw new HttpError(422, 'invalid_import', '技能名称无效。');
      const directory = resolve(project.directory, path);
      if (directory !== project.directory && !directory.startsWith(project.directory + sep)) throw new HttpError(422, 'outside_project', '技能路径超出项目。');
      // Check every source path component, not just the final file, for symlinks.
      let cursor = project.directory;
      try {
        for (const part of path === '.' ? [] : path.split('/')) {
          cursor = join(cursor, part);
          if ((await lstat(cursor)).isSymbolicLink()) throw new HttpError(422, 'unsafe_package', '不能从符号链接目录导入技能。');
        }
        const files = await readPackage(directory, project.directory);
        total += files.reduce((sum, file) => sum + (file.kind === 'file' ? bytes(file).length : 0), 0);
        if (total > MAX_PACKAGE_BYTES) throw new HttpError(413, 'import_too_large', '本次导入总大小不能超过 20 MiB，请分批导入。');
        packages.push({ path, name: value.name as string | undefined, files });
      } catch (error) {
        if (error instanceof HttpError) throw error;
        throw new HttpError(422, 'import_unavailable', '技能目录或文件不可访问，请重新扫描项目。');
      }
    }
    return this.skills.importPackages(org, project.id, packages);
  }
}
