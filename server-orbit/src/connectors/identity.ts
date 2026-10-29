import { createHash } from 'node:crypto';
import { mkdir, readFile, realpath, stat, writeFile } from 'node:fs/promises';
import { isAbsolute, join } from 'node:path';
import { HttpError } from '../errors.js';
import { executable, runProcess } from './process.js';
import type { Credentials, ExecutorId, Identity } from './types.js';

type JsonObject = Record<string, unknown>;
function object(value: unknown): JsonObject { return value && typeof value === 'object' && !Array.isArray(value) ? value as JsonObject : {}; }
function nonempty(value: unknown): value is string { return typeof value === 'string' && value.trim().length > 0; }
function email(value: unknown): string | undefined { return typeof value === 'string' && /^[^\s@\p{Cc}]{1,100}@[^\s@\p{Cc}]{1,150}$/u.test(value) ? value : undefined; }
function claims(value: unknown): JsonObject {
  if (typeof value !== 'string') return {};
  try { return object(JSON.parse(Buffer.from(value.split('.')[1], 'base64url').toString('utf8'))); } catch { return {}; }
}

async function jsonFile(path: string): Promise<JsonObject | undefined> {
  try {
    if (!(await stat(path)).isFile() || (await stat(path)).size > 512 * 1024) return undefined;
    return object(JSON.parse(await readFile(path, 'utf8')));
  } catch { return undefined; }
}

export function sourceRoot(executor: ExecutorId, home: string, env: NodeJS.ProcessEnv): string {
  const configured = executor === 'codex' ? env.CODEX_HOME : executor === 'claude' ? env.CLAUDE_CONFIG_DIR : undefined;
  if (configured && isAbsolute(configured)) return configured;
  return join(executor === 'gemini' && env.GEMINI_CLI_HOME && isAbsolute(env.GEMINI_CLI_HOME) ? env.GEMINI_CLI_HOME : home, executor === 'codex' ? '.codex' : executor === 'claude' ? '.claude' : '.gemini');
}

async function keychain(service: string, account: string | undefined, env: NodeJS.ProcessEnv): Promise<JsonObject | undefined> {
  const command = await executable(process.platform === 'darwin' ? 'security' : 'secret-tool', env);
  if (!command) return undefined;
  const args = process.platform === 'darwin' ? ['find-generic-password', '-s', service, ...(account ? ['-a', account] : []), '-w'] : ['lookup', 'service', service, ...(account ? ['username', account] : [])];
  const result = await runProcess(command, args, env, 4000);
  if (!result.ok) return undefined;
  try { return object(JSON.parse(result.output)); } catch { return undefined; }
}

/** Explicit user-requested local import: reads only the executor's auth files. */
export async function readLocalIdentity(executor: ExecutorId, root: string, originalHome: string, env: NodeJS.ProcessEnv): Promise<Identity> {
  if (executor === 'codex') {
    const canonical = await realpath(root).catch(() => root);
    const account = `cli|${createHash('sha256').update(canonical).digest('hex').slice(0, 16)}`;
    const value = await jsonFile(join(root, 'auth.json')) ?? await keychain('Codex Auth', account, env);
    if (value) {
      const tokens = object(value.tokens);
      if (nonempty(value.OPENAI_API_KEY)) return { credentials: { kind: 'files', files: [{ path: '.codex/auth.json', content: JSON.stringify({ auth_mode: 'apikey', OPENAI_API_KEY: value.OPENAI_API_KEY }) }] }, summary: 'OpenAI · 本地 API 身份', sourceRoot: root };
      if (nonempty(tokens.access_token) && nonempty(tokens.id_token) && nonempty(tokens.refresh_token)) {
        // JWT claims are display hints; native `codex login status` performs the local auth check.
        const label = email(claims(tokens.id_token).email) ?? 'Codex 本地账号';
        return { credentials: { kind: 'files', files: [{ path: '.codex/auth.json', content: JSON.stringify({ ...value, auth_mode: value.auth_mode ?? 'chatgpt' }) }] }, summary: label, sourceRoot: root };
      }
    }
  } else if (executor === 'claude') {
    const service = root === join(originalHome, '.claude') ? 'Claude Code-credentials' : `Claude Code-credentials-${createHash('sha256').update(root).digest('hex').slice(0, 8)}`;
    const value = await jsonFile(join(root, '.credentials.json')) ?? await jsonFile(join(root, 'credentials.json')) ?? (process.platform === 'darwin' ? await keychain(service, undefined, env) : undefined);
    const token = object(value?.claudeAiOauth);
    if (value && nonempty(token.accessToken)) {
      const global = await jsonFile(join(root, '.claude.json')) ?? (root === join(originalHome, '.claude') ? await jsonFile(join(originalHome, '.claude.json')) : undefined);
      const account = object(global?.oauthAccount);
      const summary = email(account.emailAddress) ?? email(claims(token.accessToken).email) ?? 'Claude Code 本地账号';
      const settings = JSON.stringify({ hasCompletedOnboarding: true, oauthAccount: { emailAddress: email(account.emailAddress), accountUuid: typeof account.accountUuid === 'string' ? account.accountUuid : undefined, organizationUuid: typeof account.organizationUuid === 'string' ? account.organizationUuid : undefined } });
      return { credentials: { kind: 'files', files: [
        { path: '.claude/.credentials.json', content: JSON.stringify({ claudeAiOauth: token }) },
        { path: '.claude.json', content: settings }, { path: '.claude/.claude.json', content: settings },
      ] }, summary, sourceRoot: root };
    }
  } else {
    const value = await jsonFile(join(root, 'oauth_creds.json'));
    if (value && (nonempty(value.access_token) || nonempty(value.refresh_token))) {
      const account = await jsonFile(join(root, 'google_accounts.json'));
      const settings = await jsonFile(join(root, 'settings.json'));
      const model = object(settings?.model);
      return { credentials: { kind: 'files', files: [
        { path: '.gemini/oauth_creds.json', content: JSON.stringify(value) },
        { path: '.gemini/settings.json', content: JSON.stringify({ security: { auth: { selectedType: 'oauth-personal' } }, ...(typeof model.name === 'string' ? { model: { name: model.name } } : {}) }) },
        ...(email(account?.active) ? [{ path: '.gemini/google_accounts.json', content: JSON.stringify({ active: account!.active }) }] : []),
      ] }, summary: email(account?.active) ?? email(claims(value.id_token).email) ?? 'Gemini 本地账号', sourceRoot: root };
    }
  }
  throw new HttpError(422, 'local_identity_missing', '未找到可导入的本地登录身份，请先在本机执行器中登录，再重新检测。');
}

export async function writeCredentials(home: string, credentials: Credentials, executor: ExecutorId, provider: string, env: NodeJS.ProcessEnv): Promise<void> {
  // These targets are produced by adapters, never accepted from browser input.
  const allowed = new Set(['.codex/auth.json', '.claude/.credentials.json', '.claude.json', '.claude/.claude.json', '.gemini/oauth_creds.json', '.gemini/settings.json', '.gemini/google_accounts.json']);
  for (const file of credentials.files ?? []) {
    if (!allowed.has(file.path)) throw new HttpError(422, 'invalid_identity', '认证文件格式不受支持。');
    await writeFile(join(home, file.path), file.content, { mode: 0o600 });
  }
  if (executor === 'codex') {
    await writeFile(join(home, '.codex/config.toml'), 'cli_auth_credentials_store = "file"\n', { mode: 0o600 });
    if (credentials.kind === 'api_key') {
      env.OPENAI_API_KEY = credentials.key;
      await writeFile(join(home, '.codex/auth.json'), JSON.stringify({ auth_mode: 'apikey', OPENAI_API_KEY: credentials.key }), { mode: 0o600 });
    }
  } else if (executor === 'claude') {
    if (credentials.kind === 'claude_token') env.CLAUDE_CODE_OAUTH_TOKEN = credentials.token;
    if (credentials.kind === 'api_key') {
      if (provider === 'deepseek') {
        env.ANTHROPIC_AUTH_TOKEN = credentials.key;
        env.ANTHROPIC_BASE_URL = 'https://api.deepseek.com/anthropic';
        env.ANTHROPIC_MODEL = 'deepseek-chat';
        env.ANTHROPIC_DEFAULT_HAIKU_MODEL = 'deepseek-chat';
        env.ANTHROPIC_DEFAULT_SONNET_MODEL = 'deepseek-chat';
        env.ANTHROPIC_DEFAULT_OPUS_MODEL = 'deepseek-chat';
        env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1';
      } else env.ANTHROPIC_API_KEY = credentials.key;
    }
    await mkdir(join(home, '.claude'), { recursive: true, mode: 0o700 });
  }
}
