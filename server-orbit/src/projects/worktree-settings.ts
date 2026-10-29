import { lstat, realpath, stat } from 'node:fs/promises';
import { basename, dirname, isAbsolute, join, resolve, sep } from 'node:path';
import type { DatabaseSync } from 'node:sqlite';
import { HttpError } from '../errors.js';
import { git } from './git.js';

export interface WorktreeSettings { base_ref: string | null; branch_template: string | null; parent_dir: string | null }
interface TaskIdentity { id: string; identifier: string; title: string }
const empty: WorktreeSettings = { base_ref: null, branch_template: null, parent_dir: null };
const sample: TaskIdentity = { id: '00000000-0000-4000-8000-000000000000', identifier: 'ORB-1', title: 'Example task' };

export function branchForTask(template: string, task: TaskIdentity): string {
  const slug = task.title.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 80).replace(/-+$/, '') || task.identifier.toLowerCase();
  const values: Record<string, string> = { 'issue.identifier': task.identifier, 'issue.id': task.id, slug };
  const result = template.replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (_match, key: string) => {
    if (!Object.hasOwn(values, key)) throw new HttpError(422, 'invalid_branch_template', '分支模板仅支持 {{issue.identifier}}、{{issue.id}} 和 {{slug}}。');
    return values[key];
  });
  if (!result || result.length > 240 || /[\s\p{Cc}{}~^:?*\\]/u.test(result) || result.includes('[') || result.includes('..') || result.includes('@{') || result === '@' || result.startsWith('-') || result.split('/').some(part => !part || part.startsWith('.') || part.endsWith('.') || part.endsWith('.lock'))) throw new HttpError(422, 'invalid_branch_template', '分支模板生成的名称不符合 Git 分支命名规则。');
  return result;
}

/** Resolves existing symlinks even when the requested directory does not exist yet. */
async function futureDirectory(path: string): Promise<string> {
  try {
    const canonical = await realpath(path);
    if (!(await stat(canonical)).isDirectory()) throw new Error();
    return canonical;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT' || await lstat(path).then(() => true, () => false) || dirname(path) === path) throw new HttpError(422, 'invalid_worktree_parent', 'Worktree 父路径不是可访问的目录。');
    return join(await futureDirectory(dirname(path)), basename(path));
  }
}
function within(root: string, path: string) { return path === root || path.startsWith(root + sep); }

export async function worktreeParent(repoRoot: string, configured: string | null) {
  const root = await realpath(repoRoot);
  const parent = await futureDirectory(configured ? resolve(root, configured) : dirname(root));
  if (within(root, parent)) throw new HttpError(422, 'invalid_worktree_parent', 'Worktree 父目录须位于主仓库之外，例如 ../aow-worktrees。');
  return parent;
}

export async function resolveBaseCommit(repoRoot: string, ref: string) {
  const result = await git(repoRoot, ['rev-parse', '--verify', '--end-of-options', `${ref}^{commit}`]);
  const sha = result.output.trim();
  if (!result.ok || !/^[0-9a-f]{40,64}$/.test(sha)) throw new HttpError(422, 'invalid_base_ref', 'Base ref 无法解析为本地提交，请检查分支、标签或提交是否存在；远程分支需先在本机获取。');
  return sha;
}

export class ProjectWorktreeSettings {
  constructor(private database: DatabaseSync) {}
  get(org: string, project: string): WorktreeSettings { const row = this.database.prepare('SELECT base_ref, branch_template, parent_dir FROM project_worktree_settings WHERE org_id = ? AND project_id = ?').get(org, project) as unknown as WorktreeSettings | undefined; return { ...empty, ...row }; }
  parse(input: unknown, previous: WorktreeSettings): WorktreeSettings {
    if (input === undefined) return previous;
    if (input === null) return { ...empty };
    if (typeof input !== 'object' || Array.isArray(input) || Object.keys(input).some(key => !Object.hasOwn(empty, key))) throw new HttpError(422, 'invalid_worktree_settings', 'Worktree 配置仅支持 Base ref、Branch template 和 Worktree parent dir。');
    const result = { ...previous };
    for (const [key, limit, code] of [['base_ref', 512, 'invalid_base_ref'], ['branch_template', 200, 'invalid_branch_template'], ['parent_dir', 4096, 'invalid_worktree_parent']] as const) {
      const value = (input as Record<string, unknown>)[key]; if (value === undefined) continue;
      if (value !== null && (typeof value !== 'string' || value.length > limit || /\p{Cc}/u.test(value))) throw new HttpError(422, code, 'Worktree 参数格式或长度无效。');
      result[key] = typeof value === 'string' ? value.trim() || null : null;
    }
    if (result.base_ref && (/\s/.test(result.base_ref) || result.base_ref.startsWith('-'))) throw new HttpError(422, 'invalid_base_ref', '请输入有效的 Git 分支、标签或提交。');
    if (result.branch_template) branchForTask(result.branch_template, sample);
    return result;
  }
  async validate(settings: WorktreeSettings, repoRoot: string) {
    if (settings.base_ref) await resolveBaseCommit(repoRoot, settings.base_ref);
    if (settings.branch_template && !(await git(repoRoot, ['check-ref-format', '--branch', branchForTask(settings.branch_template, sample)])).ok) throw new HttpError(422, 'invalid_branch_template', '分支模板生成的名称不符合 Git 分支命名规则。');
    if (settings.parent_dir) await worktreeParent(repoRoot, settings.parent_dir);
  }
  save(org: string, project: string, settings: WorktreeSettings) { this.database.prepare('INSERT INTO project_worktree_settings VALUES (?, ?, ?, ?, ?) ON CONFLICT(org_id, project_id) DO UPDATE SET base_ref = excluded.base_ref, branch_template = excluded.branch_template, parent_dir = excluded.parent_dir').run(org, project, settings.base_ref, settings.branch_template, settings.parent_dir); }
}

export async function taskWorktreePlan(repoRoot: string, org: string, task: TaskIdentity, settings: WorktreeSettings) {
  const branch = branchForTask(settings.branch_template ?? `orbit/${org.slice(0, 8)}/{{issue.identifier}}`, task);
  if (!(await git(repoRoot, ['check-ref-format', '--branch', branch])).ok) throw new HttpError(422, 'invalid_branch_template', '生成的任务分支名称无效。');
  const baseCommit = await resolveBaseCommit(repoRoot, settings.base_ref ?? 'HEAD');
  const parent = await worktreeParent(repoRoot, settings.parent_dir);
  const requested = settings.parent_dir ? join(parent, branch) : join(parent, `${basename(repoRoot)}-orbit-${org.slice(0, 8)}-${task.identifier}`);
  if (await lstat(requested).then(() => true, () => false)) throw new HttpError(409, 'workspace_exists', '预期的任务工作区目录已存在，请先明确绑定已有工作区。');
  const directory = await futureDirectory(requested);
  if (!isAbsolute(directory) || !within(parent, directory) || directory === parent || within(await realpath(repoRoot), directory)) throw new HttpError(422, 'invalid_worktree_parent', '任务工作区路径超出配置的父目录或进入了主仓库。');
  return { branch, directory, baseCommit };
}
