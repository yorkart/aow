import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { stripVTControlCharacters } from 'node:util';
import type { DatabaseSync } from 'node:sqlite';
import { HttpError } from '../errors.js';
import { ConnectorAdapters } from './adapters.js';
import { isolatedEnvironment, requireExecutor, shellQuote, executable } from './process.js';
import { readLocalIdentity } from './identity.js';
import type { AuthSession, Credentials, ExecutorId, ProviderId } from './types.js';
import { CredentialVault } from './vault.js';

interface ActiveFlow { child: ChildProcessWithoutNullStreams; home: string; output: string; timer: ReturnType<typeof setTimeout>; finished: boolean; submitted: boolean }

function loginUrl(text: string, executor: ExecutorId): string | undefined {
  for (const candidate of text.match(/https:\/\/[^\s<>]+/g) ?? []) {
    try {
      const url = new URL(candidate);
      if (url.username || url.password || url.port || url.hash || candidate.length > 4096 || /sk-ant-|access_token=/i.test(candidate)) continue;
      if (executor === 'codex' && url.origin === 'https://auth.openai.com' && /^\/codex\/device\/?$/.test(url.pathname)) return url.href;
      const single = ['response_type', 'client_id', 'state', 'code_challenge', 'code_challenge_method', 'redirect_uri'].every(key => url.searchParams.getAll(key).length === 1 && Boolean(url.searchParams.get(key)));
      const callback = url.searchParams.get('redirect_uri');
      if (executor === 'claude' && single && (url.origin + url.pathname === 'https://claude.ai/oauth/authorize' || url.origin + url.pathname === 'https://claude.com/cai/oauth/authorize') && url.searchParams.get('response_type') === 'code' && url.searchParams.get('code_challenge_method') === 'S256' && ['https://platform.claude.com/oauth/code/callback', 'https://console.anthropic.com/oauth/code/callback'].includes(callback ?? '')) return url.href;
    } catch { /* Only executor-owned official authorization URLs are surfaced. */ }
  }
  return undefined;
}

function claudeToken(text: string): string | undefined {
  const block = /Your OAuth token \(valid for 1 year\):\s*([\s\S]*?)Store this token securely\./.exec(text)?.[1];
  const token = block?.replace(/\s/g, '');
  return token && /^sk-ant-oat01-[A-Za-z0-9_-]{20,4096}$/.test(token) ? token : undefined;
}

export class ConnectorAuthFlows {
  private active = new Map<string, ActiveFlow>();
  private starting = new Map<string, number>();
  private closed = false;
  constructor(private database: DatabaseSync, private adapters: ConnectorAdapters, private vault: CredentialVault) {}

  private row(org: string, id: string): AuthSession {
    const row = this.database.prepare('SELECT * FROM connector_auth_sessions WHERE org_id = ? AND id = ?').get(org, id) as unknown as AuthSession | undefined;
    if (!row) throw new HttpError(404, 'auth_session_not_found', '授权流程不存在。');
    return row;
  }

  publicStatus(org: string, id: string) {
    let row = this.row(org, id);
    if (['starting', 'waiting', 'completing'].includes(row.status) && !this.active.has(id)) {
      this.database.prepare("UPDATE connector_auth_sessions SET status = 'error', message = '授权执行器已断开，请重新开始登录。', verification_uri = NULL, user_code = NULL WHERE id = ?").run(id);
    } else if (row.expires_at <= Date.now() && row.status !== 'cancelled') {
      this.cancel(org, id, 'expired');
    }
    row = this.row(org, id);
    return { id: row.id, provider: row.provider, executor: row.executor, status: row.status, verification_uri: row.verification_uri, user_code: row.user_code, message: row.message, account_summary: row.account_summary, expires_at: row.expires_at };
  }

  async start(org: string, provider: ProviderId, executor: ExecutorId) {
    if (this.closed) throw new HttpError(503, 'auth_unavailable', '授权服务已停止。');
    const starting = [...this.starting.values()].reduce((sum, count) => sum + count, 0);
    const activeInOrg = [...this.active.keys()].filter(id => (this.database.prepare('SELECT org_id FROM connector_auth_sessions WHERE id = ?').get(id) as { org_id: string } | undefined)?.org_id === org).length;
    if (this.active.size + starting >= 4 || activeInOrg + (this.starting.get(org) ?? 0) >= 2) throw new HttpError(429, 'auth_busy', '同时进行的授权过多，请先完成或取消已有授权。');
    this.starting.set(org, (this.starting.get(org) ?? 0) + 1);
    try { return await this.launch(org, provider, executor); }
    finally { const count = (this.starting.get(org) ?? 1) - 1; if (count) this.starting.set(org, count); else this.starting.delete(org); }
  }

  private async launch(org: string, provider: ProviderId, executor: ExecutorId) {
    const command = await requireExecutor(executor, this.adapters.env);
    if (executor === 'gemini') throw new HttpError(422, 'unsupported_login', '此执行器仅支持本地身份导入。');
    const script = executor === 'claude' ? await executable('script', this.adapters.env) : undefined;
    if (executor === 'claude' && !script) throw new HttpError(422, 'pty_unavailable', 'Claude Code 独立登录需要本机 script PTY 工具，请安装后重试。');
    const id = randomUUID();
    const home = join(this.adapters.stateDir, 'connector-auth', id, 'home');
    const env = await isolatedEnvironment(home, this.adapters.env);
    if (this.closed) { await rm(join(home, '..'), { recursive: true, force: true }); throw new HttpError(503, 'auth_unavailable', '授权服务已停止。'); }
    await mkdir(join(home, '.codex'), { recursive: true, mode: 0o700 });
    env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1'; env.DISABLE_AUTOUPDATER = '1';
    const created = new Date().toISOString();
    this.database.prepare(`INSERT INTO connector_auth_sessions (id, org_id, provider, executor, status, message, expires_at, created_at)
      VALUES (?, ?, ?, ?, 'starting', '正在启动执行器登录…', ?, ?)`).run(id, org, provider, executor, Date.now() + 10 * 60_000, created);
    const scriptCommand = `stty cols 4096 rows 50; exec ${shellQuote(command)} setup-token`;
    const args = executor === 'codex' ? ['-c', 'cli_auth_credentials_store="file"', 'login', '--device-auth'] : process.platform === 'darwin' ? ['-q', '/dev/null', '/bin/sh', '-c', scriptCommand] : ['-q', '-e', '-f', '-c', scriptCommand, '/dev/null'];
    const child = spawn(executor === 'codex' ? command : script!, args, { env, cwd: home, stdio: ['pipe', 'pipe', 'pipe'], detached: true });
    const flow: ActiveFlow = { child, home, output: '', finished: false, submitted: false, timer: setTimeout(() => { this.cancel(org, id, 'expired'); }, 10 * 60_000) };
    flow.timer.unref();
    this.active.set(id, flow);
    const receive = (chunk: Buffer) => {
      if (flow.finished || this.closed) return;
      if (flow.output.length + chunk.length > 256 * 1024) { void this.finish(org, id, false); return; }
      flow.output += chunk.toString('utf8');
      // Some CLI versions put the full URL only in an OSC-8 terminal hyperlink.
      const escape = String.fromCharCode(27); const bell = String.fromCharCode(7);
      const links = flow.output.split(`${escape}]8;;`).slice(1).map(part => part.split(escape)[0].split(bell)[0]);
      const text = stripVTControlCharacters(flow.output) + '\n' + links.join('\n');
      const url = loginUrl(text, executor);
      const code = executor === 'codex' ? text.replace(/https:\/\/\S+/g, '').match(/\b[A-Z0-9]{4,6}-[A-Z0-9]{4,6}\b/)?.[0] : null;
      if (url && (executor === 'claude' || code) && !flow.submitted) this.database.prepare("UPDATE connector_auth_sessions SET status = 'waiting', verification_uri = ?, user_code = ?, message = ? WHERE id = ?")
        .run(url, code ?? null, executor === 'codex' ? '在官方页面输入设备码，完成后等待验证。' : '打开官方授权页面，完成后回填授权码。', id);
    };
    child.stdout.on('data', receive); child.stderr.on('data', receive);
    child.stdin.on('error', () => {});
    child.on('error', () => { void this.finish(org, id, false); });
    child.on('close', code => { void this.finish(org, id, code === 0); });
    return this.publicStatus(org, id);
  }

  async submit(org: string, id: string, value: unknown) {
    const row = this.row(org, id);
    const flow = this.active.get(id);
    if (row.executor !== 'claude' || row.status !== 'waiting' || !flow || flow.submitted || row.expires_at <= Date.now()) throw new HttpError(409, 'auth_not_waiting', '此授权流程不能接收授权码，请重新开始登录。');
    if (typeof value !== 'string' || !/^[A-Za-z0-9._~+/#=-]{1,2048}$/.test(value)) throw new HttpError(422, 'invalid_auth_code', '请输入官方页面提供的授权码。');
    flow.submitted = true;
    this.database.prepare("UPDATE connector_auth_sessions SET status = 'completing', message = '正在由执行器验证授权码…' WHERE id = ?").run(id);
    flow.child.stdin.write(value);
    await new Promise(resolve => setTimeout(resolve, 40));
    if (!flow.finished) flow.child.stdin.write('\r');
    return this.publicStatus(org, id);
  }

  private stop(flow: ActiveFlow) {
    clearTimeout(flow.timer);
    try { if (flow.child.pid) process.kill(-flow.child.pid, 'SIGTERM'); } catch { flow.child.kill('SIGTERM'); }
    flow.child.stdin.destroy();
    const kill = setTimeout(() => { try { if (flow.child.pid && flow.child.exitCode === null && flow.child.signalCode === null) process.kill(-flow.child.pid, 'SIGKILL'); } catch { /* Already exited. */ } }, 1000); kill.unref();
  }

  private async finish(org: string, id: string, success: boolean) {
    const flow = this.active.get(id);
    if (!flow || flow.finished) return;
    flow.finished = true; clearTimeout(flow.timer);
    try {
      if (this.closed) return;
      const row = this.row(org, id);
      if (['cancelled', 'expired'].includes(row.status)) return;
      let credentials: Credentials | undefined; let summary = '';
      if (success && row.executor === 'codex') {
        const identity = await readLocalIdentity('codex', join(flow.home, '.codex'), flow.home, await isolatedEnvironment(flow.home, this.adapters.env));
        credentials = identity.credentials; summary = identity.summary;
      } else if (success && row.executor === 'claude') {
        const token = claudeToken(stripVTControlCharacters(flow.output));
        if (token) { credentials = { kind: 'claude_token', token }; summary = 'Claude Code · 独立登录'; }
      }
      if (!this.closed) {
        if (credentials) this.database.prepare("UPDATE connector_auth_sessions SET status = 'complete', credentials = ?, account_summary = ?, message = '授权完成，可保存并连接。', verification_uri = NULL, user_code = NULL WHERE id = ? AND status NOT IN ('cancelled', 'expired')")
          .run(this.vault.seal(credentials, `${org}:auth:${id}`), summary, id);
        else this.database.prepare("UPDATE connector_auth_sessions SET status = 'error', message = '执行器未完成有效授权，请重试。', verification_uri = NULL, user_code = NULL WHERE id = ? AND status NOT IN ('cancelled', 'expired')").run(id);
      }
    } catch {
      if (!this.closed) this.database.prepare("UPDATE connector_auth_sessions SET status = 'error', message = '无法读取有效的授权结果，请重试。', verification_uri = NULL, user_code = NULL WHERE id = ? AND status NOT IN ('cancelled', 'expired')").run(id);
    } finally {
      flow.output = ''; this.stop(flow); this.active.delete(id);
      await rm(join(flow.home, '..'), { recursive: true, force: true }).catch(() => {});
    }
  }

  cancel(org: string, id: string, state: 'cancelled' | 'expired' = 'cancelled') {
    this.row(org, id);
    this.database.prepare('UPDATE connector_auth_sessions SET status = ?, message = ?, credentials = NULL, verification_uri = NULL, user_code = NULL WHERE id = ?').run(state, state === 'expired' ? '授权已过期，请重新登录。' : '授权已取消。', id);
    const flow = this.active.get(id);
    if (flow) { flow.finished = true; flow.output = ''; this.stop(flow); this.active.delete(id); void rm(join(flow.home, '..'), { recursive: true, force: true }).catch(() => {}); }
    return { status: state };
  }

  credentials(org: string, id: string, provider: ProviderId, executor: ExecutorId) {
    const row = this.row(org, id);
    if (row.provider !== provider || row.executor !== executor || row.status !== 'complete' || !row.credentials || row.expires_at <= Date.now()) throw new HttpError(409, 'invalid_auth_session', '授权结果已失效或与当前连接不匹配，请重新登录。');
    return { credentials: this.vault.open(row.credentials, `${org}:auth:${id}`), summary: row.account_summary };
  }

  close() {
    for (const [id] of this.active) { const row = this.database.prepare('SELECT org_id FROM connector_auth_sessions WHERE id = ?').get(id) as { org_id: string }; this.cancel(row.org_id, id); }
    this.closed = true;
  }
}
