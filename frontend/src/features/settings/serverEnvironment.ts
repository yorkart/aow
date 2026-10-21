interface EnvironmentField {
  key: string;
  description: string;
  example: string;
}

export const serverEnvironmentGroups: { title: string; fields: EnvironmentField[] }[] = [
  { title: '服务配置', fields: [
    { key: 'AOW_SERVER_HOST', description: '服务监听地址，默认 127.0.0.1。0.0.0.0 监听全部 IPv4 接口，:: 监听 IPv6。', example: '127.0.0.1' },
    { key: 'AOW_SERVER_PORT', description: '服务监听端口，默认 8282。', example: '8282' },
    { key: 'AOW_SERVER_STATE_DIR', description: '服务数据目录，使用绝对路径；优先于 AOW_STATE_DIR。', example: '/srv/aow/state' },
    { key: 'AOW_STATE_DIR', description: 'AoW 数据目录；未设置时使用 XDG_STATE_HOME 下的 aow，或 ~/.local/state/aow。', example: '/srv/aow/state' },
    { key: 'AOW_TERMINALD_SOCKET', description: '连接 terminald 的 Unix socket 绝对路径，须与 terminald 的监听路径一致。', example: '/run/user/1000/aow-terminald/terminald.sock' },
    { key: 'AOW_BASE_PATH', description: '反向代理部署的 URL 子路径，默认根路径 /。', example: '/tools/aow' },
  ] },
  { title: '登录会话', fields: [
    { key: 'AOW_AUTH_SECURE_COOKIE', description: '是否仅通过 HTTPS 发送登录 Cookie。填写 true 或 false，默认 false；不为服务启用 TLS。', example: 'true' },
    { key: 'AOW_AUTH_SESSION_MAX_AGE_SECONDS', description: '登录后的最长有效时间，活动不会延长。单位秒，正整数；默认 43200（12 小时）。', example: '43200' },
    { key: 'AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS', description: '无有效活动多久后登录过期，活动会重新计时。单位秒，正整数；默认 1800（30 分钟）。任一会话时限达到即过期。', example: '1800' },
  ] },
  { title: '基础运行环境', fields: [
    { key: 'XDG_STATE_HOME', description: '用户状态数据的根目录，默认 ~/.local/state；未指定 AoW 数据目录时使用其 aow 子目录。填写绝对路径。', example: '/home/aow/.local/state' },
    { key: 'XDG_RUNTIME_DIR', description: '用户运行时目录，填写绝对路径；默认 terminald socket 位于其 aow-terminald 子目录。未设置时使用 /tmp/aow-terminald-用户ID。', example: '/run/user/1000' },
    { key: 'PATH', description: '服务进程查找程序的目录，用冒号分隔绝对路径。与上方单独保存的 Agent 执行 PATH 不同。', example: '/home/aow/.local/bin:/usr/local/bin:/usr/bin:/bin' },
    { key: 'SHELL', description: '服务环境中的 shell 可执行文件绝对路径。', example: '/bin/zsh' },
    { key: 'LANG', description: '默认语言和字符编码，应使用系统已安装的 UTF-8 locale。', example: 'en_US.UTF-8' },
    { key: 'LC_ALL', description: '覆盖所有语言和字符编码设置，优先于 LANG 和其他 LC_* 变量。', example: 'en_US.UTF-8' },
    { key: 'LC_CTYPE', description: '字符分类和字符编码，覆盖 LANG；LC_ALL 的优先级更高。', example: 'en_US.UTF-8' },
  ] },
  { title: '代理配置', fields: [
    ...[
      { key: 'HTTP_PROXY', description: 'HTTP 请求使用的代理地址。', example: 'http://127.0.0.1:7890' },
      { key: 'HTTPS_PROXY', description: 'HTTPS 请求使用的代理地址；HTTP 代理的地址仍可使用 http://。', example: 'http://127.0.0.1:7890' },
      { key: 'ALL_PROXY', description: '通用代理地址，协议支持和优先级取决于客户端。', example: 'socks5://127.0.0.1:7890' },
      { key: 'NO_PROXY', description: '不经过代理的主机、域名或 IP，用逗号分隔。', example: 'localhost,127.0.0.1,::1' },
    ].flatMap(field => [field, { ...field, key: field.key.toLowerCase(), description: `${field.description}小写形式，供读取小写变量的客户端使用，与大写项独立保存。` }]),
  ] },
];

export const serverEnvironmentFields = serverEnvironmentGroups.flatMap(group => group.fields);
export type ServerEnvironmentValues = Record<string, string>;
const keys = new Set(serverEnvironmentFields.map(field => field.key));
const proxyKey = /^(http|https|all|no)_proxy$/i;
const assignment = /^([ \t]*([A-Za-z_][A-Za-z0-9_]*)[ \t]*=[ \t]*)(.*?)([ \t]*)$/s;

function continues(value: string) {
  const quote = value[0];
  if (quote === "'" || quote === '"') {
    for (let index = 1; index < value.length; index++) {
      if (quote === '"' && value[index] === '\\') index++;
      else if (value[index] === quote) return false;
    }
    return true;
  }
  return (value.match(/\\+$/)?.[0].length ?? 0) % 2 === 1;
}

function lines(content: string, platform: string) {
  const physical = content.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const entries: { raw: string; ending: string; match: RegExpMatchArray | null }[] = [];
  for (let index = 0; index < physical.length; index++) {
    let raw = physical[index];
    for (;;) {
      const ending = raw.endsWith('\r\n') ? '\r\n' : raw.endsWith('\n') ? '\n' : '';
      const text = ending ? raw.slice(0, -ending.length) : raw;
      const match = text.match(assignment);
      // Keep systemd multiline entries together, including unmanaged variables,
      // so a line inside a quoted value is never edited as another assignment.
      if (platform === 'linux' && match && !proxyKey.test(match[2]) && continues(match[3]) && index + 1 < physical.length) {
        raw += physical[++index];
      } else {
        entries.push({ raw, ending, match });
        break;
      }
    }
  }
  return entries;
}

function valueOf(raw: string, systemd: boolean) {
  const quoted = raw.match(/^(['"])(.*)\1$/s);
  const value = quoted ? quoted[2] : raw;
  // launchd and the proxy launcher read literal values, without shell expansion.
  if (!systemd || quoted?.[1] === "'") return value;
  const joined = value.replace(/\\\r?\n/g, '');
  return quoted ? joined.replace(/\\([\\"`$])/g, '$1') : joined.replace(/\\(.)/g, '$1');
}

export function readServerEnvironment(content: string, platform: string): ServerEnvironmentValues {
  const values: ServerEnvironmentValues = {};
  for (const { match } of lines(content, platform)) {
    if (match && keys.has(match[2])) values[match[2]] = valueOf(match[3], platform === 'linux' && !proxyKey.test(match[2]));
  }
  return values;
}

export function validateServerEnvironment(values: ServerEnvironmentValues): ServerEnvironmentValues {
  const errors: ServerEnvironmentValues = {};
  for (const { key } of serverEnvironmentFields) {
    const value = values[key] ?? '';
    if (!value) continue;
    if (/[\r\n\0]/.test(value)) errors[key] = '请填写单行值，不能包含换行或空字符。';
    else if (key === 'AOW_SERVER_PORT' && (!/^\d+$/.test(value) || Number(value) > 65535)) errors[key] = '端口必须是 0–65535 的整数。';
    else if (key === 'AOW_AUTH_SECURE_COOKIE' && !['true', 'false'].includes(value)) errors[key] = '请填写 true 或 false。';
    else if (key.startsWith('AOW_AUTH_SESSION_') && (!/^\d+$/.test(value) || BigInt(value) === 0n || BigInt(value) > 18446744073709551615n)) errors[key] = '请填写大于 0 的整数秒，最大为 18446744073709551615。';
  }
  return errors;
}

function encodeValue(value: string, previous: string, systemd: boolean) {
  const quote = previous.match(/^(['"])(.*)\1$/s)?.[1];
  if (quote === "'" && !value.includes("'")) return `'${value}'`;
  if (!quote && !/[\s'"\\]/.test(value)) return value;
  if (quote !== '"' && !value.includes("'")) return `'${value}'`;
  return `"${systemd ? value.replace(/[\\"`$]/g, '\\$&') : value}"`;
}

export function writeServerEnvironment(content: string, values: ServerEnvironmentValues, platform: string): string {
  const original = readServerEnvironment(content, platform);
  const changed = new Set(serverEnvironmentFields.filter(({ key }) => (values[key] ?? '') !== (original[key] ?? '')).map(({ key }) => key));
  const errors = validateServerEnvironment(Object.fromEntries([...changed].map(key => [key, values[key]])));
  if (Object.keys(errors).length) throw new Error(Object.entries(errors).map(([key, error]) => `${key}：${error}`).join('\n'));
  const entries = lines(content, platform);
  const last = new Map<string, number>();
  entries.forEach(({ match }, index) => { if (match) last.set(match[2], index); });
  // Only rewrite edited keys. Removing every duplicate prevents an older value resurfacing.
  let result = entries.map(({ raw, ending, match }, index) => {
    if (!match || !changed.has(match[2])) return raw;
    const key = match[2];
    const value = values[key] ?? '';
    if (!value || last.get(key) !== index) return '';
    return `${match[1]}${encodeValue(value, match[3], platform === 'linux' && !proxyKey.test(key))}${match[4]}${ending}`;
  }).join('');
  const ending = content.includes('\r\n') ? '\r\n' : '\n';
  for (const key of changed) {
    const value = values[key] ?? '';
    if (last.has(key) || !value) continue;
    if (result && !result.endsWith('\n')) result += ending;
    result += `${key}=${encodeValue(value, '', platform === 'linux' && !proxyKey.test(key))}${ending}`;
  }
  return result;
}
