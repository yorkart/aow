import { spawn } from 'node:child_process';
import { constants } from 'node:fs';
import { access, mkdir } from 'node:fs/promises';
import { delimiter, isAbsolute, join } from 'node:path';
import { HttpError } from '../errors.js';
import type { ExecutorId } from './types.js';

export const commands: Record<ExecutorId, string> = { codex: 'codex', claude: 'claude', gemini: 'gemini' };
const privateVariables = new Set([
  'OPENAI_API_KEY', 'CODEX_API_KEY', 'CODEX_ACCESS_TOKEN', 'OPENAI_BASE_URL',
  'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL', 'ANTHROPIC_MODEL',
  'ANTHROPIC_DEFAULT_HAIKU_MODEL', 'ANTHROPIC_DEFAULT_SONNET_MODEL', 'ANTHROPIC_DEFAULT_OPUS_MODEL',
  'CLAUDE_CODE_OAUTH_TOKEN', 'CLAUDE_CONFIG_DIR', 'CODEX_HOME',
  'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX', 'CLAUDE_CODE_USE_FOUNDRY',
  'ANTHROPIC_FOUNDRY_API_KEY', 'ANTHROPIC_FOUNDRY_BASE_URL', 'GOOGLE_GENAI_USE_VERTEXAI',
  'GEMINI_API_KEY', 'GOOGLE_API_KEY', 'GOOGLE_APPLICATION_CREDENTIALS', 'GOOGLE_CLOUD_ACCESS_TOKEN', 'GEMINI_CLI_HOME',
]);

export async function executable(command: string, env: NodeJS.ProcessEnv): Promise<string | undefined> {
  for (const directory of (env.PATH ?? '').split(delimiter)) {
    if (!isAbsolute(directory)) continue;
    const path = join(directory, command);
    if (await access(path, constants.X_OK).then(() => true, () => false)) return path;
  }
  return undefined;
}

export async function isolatedEnvironment(home: string, source: NodeJS.ProcessEnv): Promise<NodeJS.ProcessEnv> {
  const env = Object.fromEntries(Object.entries(source).filter(([key]) => !privateVariables.has(key)));
  Object.assign(env, {
    HOME: home, USERPROFILE: home, XDG_CONFIG_HOME: join(home, '.config'), XDG_CACHE_HOME: join(home, '.cache'),
    XDG_DATA_HOME: join(home, '.local/share'), XDG_STATE_HOME: join(home, '.local/state'),
    CODEX_HOME: join(home, '.codex'), CLAUDE_CONFIG_DIR: join(home, '.claude'), GEMINI_CLI_HOME: home,
    NO_COLOR: '1', TERM: 'xterm-256color', BROWSER: 'false', NO_BROWSER: '1',
  });
  for (const directory of [home, env.CODEX_HOME!, env.CLAUDE_CONFIG_DIR!, join(home, '.gemini'), env.XDG_CONFIG_HOME!, env.XDG_CACHE_HOME!]) await mkdir(directory, { recursive: true, mode: 0o700 });
  return env;
}

export function runProcess(command: string, args: string[], env: NodeJS.ProcessEnv, timeout = 5000, input?: string): Promise<{ ok: boolean; output: string }> {
  return new Promise(resolve => {
    let output = ''; let settled = false;
    const child = spawn(command, args, { env, cwd: env.HOME, stdio: ['pipe', 'pipe', 'pipe'] });
    const timer = setTimeout(() => { child.kill('SIGKILL'); finish(false); }, timeout);
    const finish = (ok: boolean) => { if (settled) return; settled = true; clearTimeout(timer); resolve({ ok, output }); };
    const receive = (chunk: Buffer) => {
      if (output.length + chunk.length > 256 * 1024) { child.kill('SIGKILL'); finish(false); return; }
      output += chunk.toString('utf8');
    };
    child.stdout.on('data', receive); child.stderr.on('data', receive);
    child.on('error', () => finish(false)); child.on('close', code => finish(code === 0));
    child.stdin.on('error', () => {});
    child.stdin.end(input);
  });
}

export async function requireExecutor(executor: ExecutorId, env: NodeJS.ProcessEnv): Promise<string> {
  const command = await executable(commands[executor], env);
  if (!command) throw new HttpError(422, 'executor_unavailable', `本机未安装 ${commands[executor]}，请安装执行器后重试。`);
  return command;
}

export function shellQuote(value: string): string { return `'${value.replace(/'/g, `'\\''`)}'`; }
