import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { access, realpath, stat } from 'node:fs/promises';
import { isAbsolute, resolve } from 'node:path';
import { HttpError } from '../errors.js';

export async function localDirectory(value: unknown): Promise<string> {
  if (typeof value !== 'string' || !isAbsolute(value) || value.length > 4096 || /\p{Cc}/u.test(value)) throw new HttpError(422, 'invalid_codebase', '请输入本地目录的绝对路径。');
  try {
    const directory = await realpath(value);
    if (!(await stat(directory)).isDirectory()) throw new Error();
    await access(directory, constants.R_OK | constants.X_OK);
    return directory;
  } catch { throw new HttpError(422, 'invalid_codebase', '目录不存在、不是目录或无法访问，请检查运行 Orbit 的机器上的路径。'); }
}

export function git(directory: string, args: string[], timeout = 8000): Promise<{ ok: boolean; output: string }> {
  return new Promise(resolveResult => {
    const env = Object.fromEntries(Object.entries(process.env).filter(([name]) => !name.startsWith('GIT_')));
    Object.assign(env, { GIT_TERMINAL_PROMPT: '0', GIT_OPTIONAL_LOCKS: '0' });
    const child = spawn('git', ['-c', 'core.fsmonitor=false', '-c', 'core.hooksPath=/dev/null', '-C', directory, ...args], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    const chunks: Buffer[] = []; let size = 0; let done = false;
    const finish = (ok: boolean) => { if (done) return; done = true; clearTimeout(timer); resolveResult({ ok, output: Buffer.concat(chunks).toString('utf8') }); };
    const timer = setTimeout(() => { child.kill('SIGKILL'); finish(false); }, timeout);
    child.stdout.on('data', (chunk: Buffer) => { if (done) return; size += chunk.length; if (size > 4 * 1024 * 1024) { child.kill('SIGKILL'); finish(false); } else chunks.push(chunk); });
    // Drain stderr, but do not expose native errors or repository configuration to the browser.
    child.stderr.resume(); child.on('error', () => finish(false)); child.on('close', status => finish(status === 0));
  });
}

export interface GitWorkspace { directory: string; branch: string | null; head: string | null; locked: boolean; prunable: boolean }
export function parseWorktrees(text: string): GitWorkspace[] {
  const result: GitWorkspace[] = [];
  for (const record of text.split('\0\0')) {
    const fields = record.split('\0'); const path = fields.find(value => value.startsWith('worktree '))?.slice(9);
    if (!path || !isAbsolute(path) || fields.includes('bare')) continue;
    const branch = fields.find(value => value.startsWith('branch '))?.slice(7).replace(/^refs\/heads\//, '') ?? null;
    result.push({ directory: resolve(path), branch, head: fields.find(value => value.startsWith('HEAD '))?.slice(5) ?? null, locked: fields.some(value => value === 'locked' || value.startsWith('locked ')), prunable: fields.some(value => value === 'prunable' || value.startsWith('prunable ')) });
  }
  return result;
}

export async function gitInfo(directory: string) {
  const top = await git(directory, ['rev-parse', '--show-toplevel']);
  if (!top.ok) return { is_git: false, root: null, branch: null, head: null, worktrees: [] as GitWorkspace[], warning: null as string | null };
  const root = top.output.trimEnd();
  const [branch, head, worktrees] = await Promise.all([git(directory, ['symbolic-ref', '--quiet', '--short', 'HEAD']), git(directory, ['rev-parse', '--verify', 'HEAD']), git(directory, ['worktree', 'list', '--porcelain', '-z'])]);
  return { is_git: true, root, branch: branch.ok ? branch.output.trimEnd() : null, head: head.ok ? head.output.trimEnd() : null, worktrees: worktrees.ok ? parseWorktrees(worktrees.output) : [], warning: worktrees.ok ? null : '无法读取 Git worktree 列表，请检查本地仓库。' };
}
