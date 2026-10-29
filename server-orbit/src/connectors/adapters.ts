import { homedir } from 'node:os';
import { mkdtemp, mkdir, rm } from 'node:fs/promises';
import { join } from 'node:path';
import { EnvHttpProxyAgent, fetch as proxyFetch } from 'undici';
import { HttpError } from '../errors.js';
import { executable, isolatedEnvironment, requireExecutor, runProcess } from './process.js';
import { readLocalIdentity, sourceRoot, writeCredentials } from './identity.js';
import type { AuthMethod, CheckResult, Credentials, ExecutorId, Identity, ProviderId, ProviderInfo } from './types.js';

export interface AdapterOptions { localHome?: string; env?: NodeJS.ProcessEnv; fetch?: typeof globalThis.fetch }
const endpoints: Record<'openai' | 'anthropic' | 'deepseek', string> = {
  openai: 'https://api.openai.com/v1/models', anthropic: 'https://api.anthropic.com/v1/models', deepseek: 'https://api.deepseek.com/models',
};
const definitions: ProviderInfo[] = [
  { id: 'openai', name: 'OpenAI', executors: ['codex'], auth_methods: ['local', 'independent', 'api_key'] },
  { id: 'anthropic', name: 'Anthropic', executors: ['claude'], auth_methods: ['local', 'independent', 'api_key'] },
  { id: 'deepseek', name: 'DeepSeek', executors: ['claude'], auth_methods: ['api_key'] },
  { id: 'gemini', name: 'Google Gemini', executors: ['gemini'], auth_methods: ['local'] },
];

export class ConnectorAdapters {
  readonly env: NodeJS.ProcessEnv;
  readonly localHome: string;
  private proxy = new EnvHttpProxyAgent();
  private fetcher: typeof globalThis.fetch;
  constructor(readonly stateDir: string, options: AdapterOptions = {}) {
    this.env = options.env ?? process.env; this.localHome = options.localHome ?? homedir();
    this.fetcher = options.fetch ?? (async (input, init) => await proxyFetch(String(input), { ...init, dispatcher: this.proxy } as Parameters<typeof proxyFetch>[1]) as unknown as Response);
  }

  async catalog() {
    const executors = await Promise.all(([
      { id: 'codex', name: 'Codex' }, { id: 'claude', name: 'Claude Code' }, { id: 'gemini', name: 'Gemini CLI' },
    ] as const).map(async item => ({ ...item, installed: Boolean(await executable(item.id === 'claude' ? 'claude' : item.id, this.env)) })));
    const providers = definitions.filter(item => item.id !== 'gemini' || executors.find(executor => executor.id === 'gemini')?.installed);
    return { providers, executors };
  }

  async validate(provider: unknown, executor: unknown, method: unknown): Promise<{ provider: ProviderId; executor: ExecutorId; method: AuthMethod }> {
    const selected = definitions.find(item => item.id === provider);
    if (!selected || !selected.executors.includes(executor as ExecutorId) || !selected.auth_methods.includes(method as AuthMethod)) throw new HttpError(422, 'unsupported_connection', '此 Provider、执行器或认证方式组合不受支持。');
    return { provider: selected.id, executor: executor as ExecutorId, method: method as AuthMethod };
  }

  async scratch(): Promise<string> {
    const root = join(this.stateDir, 'connector-work'); await mkdir(root, { recursive: true, mode: 0o700 });
    return mkdtemp(join(root, 'identity-'));
  }

  async local(executor: ExecutorId, root?: string): Promise<Identity> {
    await requireExecutor(executor, this.env);
    return readLocalIdentity(executor, root ?? sourceRoot(executor, this.localHome, this.env), this.localHome, this.env);
  }

  async check(executor: ExecutorId, provider: ProviderId, identity: Identity): Promise<CheckResult> {
    const command = await requireExecutor(executor, this.env);
    const home = await this.scratch();
    try {
      const env = await isolatedEnvironment(home, this.env);
      await writeCredentials(home, identity.credentials, executor, provider, env);
      const version = await runProcess(command, ['--version'], env, 4000);
      if (!version.ok) return { status: 'error', message: '执行器无法启动，请检查本机安装和运行环境。' };
      if (identity.credentials.kind === 'api_key') return this.checkKey(provider, identity.credentials.key ?? '');
      if (executor === 'codex') {
        const status = await runProcess(command, ['login', 'status'], env);
        if (!status.ok) return { status: 'auth_required', message: 'Codex 未认可此身份，请在本机重新登录或使用独立登录。' };
      } else if (executor === 'claude') {
        const status = await runProcess(command, ['auth', 'status'], env);
        let authenticated = false;
        try { authenticated = (JSON.parse(status.output) as { loggedIn?: boolean }).loggedIn === true; } catch { /* Native auth status must return JSON. */ }
        if (!status.ok || !authenticated) return { status: 'auth_required', message: 'Claude Code 未认可此身份，请重新登录，并确认 CLI 支持 auth status。' };
      }
      return { status: 'available', message: executor === 'gemini' ? '已检测到 Gemini 本地 OAuth 身份和执行器，执行时将复制最新身份。' : '已确认执行器可用，并识别到独立环境中的登录身份。', summary: identity.summary };
    } finally { await rm(home, { recursive: true, force: true }); }
  }

  async checkKey(provider: ProviderId, key: string): Promise<CheckResult> {
    if (!key || key.length > 8192 || /[\s\p{Cc}]/u.test(key) || provider === 'gemini') return { status: 'auth_required', message: '请输入有效的 API Key。' };
    try {
      const response = await this.fetcher(endpoints[provider], {
        method: 'GET', signal: AbortSignal.timeout(8000), redirect: 'error',
        headers: provider === 'anthropic' ? { 'x-api-key': key, 'anthropic-version': '2023-06-01' } : { Authorization: `Bearer ${key}` },
      });
      if (!response.ok) { await response.body?.cancel(); return { status: [401, 403].includes(response.status) ? 'auth_required' : 'error', message: `Provider 验证失败（HTTP ${response.status}），请检查密钥和账号权限。` }; }
      const reader = response.body?.getReader();
      if (!reader) return { status: 'error', message: 'Provider 未返回有效的模型列表。' };
      const decoder = new TextDecoder(); let text = ''; let size = 0;
      try {
        for (;;) {
          const chunk = await reader.read(); if (chunk.done) break;
          size += chunk.value.length;
          if (size > 2 * 1024 * 1024) { await reader.cancel(); return { status: 'error', message: 'Provider 返回的模型列表过大。' }; }
          text += decoder.decode(chunk.value, { stream: true });
        }
        text += decoder.decode();
      } finally { reader.releaseLock(); }
      const value = JSON.parse(text) as { data?: { id?: unknown }[] };
      if (!Array.isArray(value.data)) return { status: 'error', message: 'Provider 未返回有效的模型列表。' };
      return { status: 'available', message: 'API Key 已通过 Provider 模型接口验证。', summary: `${provider === 'openai' ? 'OpenAI' : provider === 'anthropic' ? 'Anthropic' : 'DeepSeek'} · API Key`, models: value.data.flatMap(model => typeof model.id === 'string' ? [model.id] : []).slice(0, 500) };
    } catch { return { status: 'error', message: '无法完成 Provider 验证，请检查网络连接后重试。' }; }
  }

  async close() { await this.proxy.close(); }
}

export function apiCredentials(key: unknown): Credentials {
  if (typeof key !== 'string' || !key.trim() || key.length > 8192 || /[\s\p{Cc}]/u.test(key)) throw new HttpError(422, 'invalid_api_key', '请输入有效的 API Key。');
  return { kind: 'api_key', key };
}
