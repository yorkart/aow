import { existsSync, lstatSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { delimiter, join, resolve } from 'node:path';

// Generate data, never shell source: paths/config values may contain quotes,
// whitespace, XML characters or dollar signs.
const [component, runtime, destination, nodePath] = process.argv.slice(2);
if (!['server', 'terminald'].includes(component) || !runtime?.startsWith('/') || !destination) {
  throw new Error('Usage: launchd-service.mjs server|terminald RUNTIME PLIST');
}
const home = process.env.HOME;
const environment = {
  HOME: home,
  SHELL: process.env.SHELL || '/bin/zsh',
  PATH: [...new Set([process.env.AOW_USER_BIN_DIR || join(home, '.local/bin'),
    ...process.env.PATH.split(delimiter), '/opt/homebrew/bin', '/usr/local/bin', '/usr/bin', '/bin', '/usr/sbin', '/sbin'])].join(delimiter),
  AOW_RUNTIME_ROOT: runtime,
};
if (process.env.USER) environment.USER = process.env.USER;
// Match server_state_dir's environment > server.env > state-dir defaults.
try {
  for (const line of readFileSync(join(home, '.config/aow/server.env'), 'utf8').split(/\r?\n/)) {
    if (!line.trim() || line.trimStart().startsWith('#')) continue;
    const match = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*?)\s*$/);
    if (!match) throw new Error('Unsupported server.env line; use KEY=value or KEY="value"');
    const [, key, raw] = match;
    environment[key] = raw.replace(/^(['"])(.*)\1$/, '$2');
  }
} catch (error) {
  if (error.code !== 'ENOENT') throw error;
}
for (const key of ['AOW_SERVER_HOST', 'AOW_SERVER_PORT', 'AOW_SERVER_STATE_DIR',
  'AOW_STATE_DIR', 'AOW_TERMINALD_SOCKET', 'AOW_BASE_PATH', 'XDG_STATE_HOME', 'XDG_RUNTIME_DIR']) {
  if (process.env[key]) environment[key] = process.env[key];
}
// These paths define the installation and cannot be redirected by server.env.
environment.HOME = home;
environment.AOW_RUNTIME_ROOT = runtime;
delete environment.AOW_SERVICE_LOG;
environment.AOW_LOG_MODE = 'unified';
if (component === 'terminald') {
  delete environment.AOW_TERMINALD_VT_WORKER;
  const nodeLink = join(runtime, 'node');
  const marker = join(runtime, '.aow-terminald-node-target');
  let info;
  try { info = lstatSync(nodeLink); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (info && (!info.isSymbolicLink() || !existsSync(marker))) {
    throw new Error('Refusing to replace an unmanaged Node runtime');
  }
  if (!info && existsSync(marker)) throw new Error('Refusing to reuse an orphaned Node marker');
  if (info) {
    const { readlinkSync } = await import('node:fs');
    if (readlinkSync(nodeLink) !== readFileSync(marker, 'utf8').trimEnd()) {
      throw new Error('Node symlink does not match installer marker');
    }
  }
  if (Number(process.versions.node.split('.')[0]) < 20) throw new Error('Node.js 20+ is required');
  const stage = resolve(destination, '..');
  if (!nodePath?.startsWith('/')) throw new Error('An absolute Node.js path is required');
  symlinkSync(nodePath, join(stage, 'node'));
  writeFileSync(join(stage, '.aow-terminald-node-target'), `${nodePath}\n`, { mode: 0o600 });
}
function xml(value) {
  const text = String(value);
  if (/[\x00-\x08\x0b\x0c\x0e-\x1f]/.test(text)) throw new Error('Invalid control character in launchd configuration');
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;').replaceAll("'", '&apos;');
}
const plist = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>org.aow.${component}</string>
<key>ProgramArguments</key><array><string>${xml(join(runtime, 'bin', `aow-${component}`))}</string></array>
<key>EnvironmentVariables</key><dict>${Object.entries(environment).map(([key, value]) => `<key>${xml(key)}</key><string>${xml(value)}</string>`).join('')}</dict>
<key>WorkingDirectory</key><string>${xml(home)}</string>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
<key>ThrottleInterval</key><integer>1</integer>
<key>Umask</key><integer>63</integer>
<key>StandardOutPath</key><string>/dev/null</string>
<key>StandardErrorPath</key><string>/dev/null</string>
</dict></plist>
`;
writeFileSync(destination, plist, { mode: 0o600 });
// Node's HTTP hostname option expects IPv6 literals without URL brackets.
const host = (environment.AOW_SERVER_HOST || '127.0.0.1').replace(/^\[([^\]]+)\]$/, '$1');
writeFileSync(resolve(destination, '../health.json'), JSON.stringify({
  component,
  host: host === '0.0.0.0' ? '127.0.0.1' : host === '::' ? '::1' : host,
  port: Number(environment.AOW_SERVER_PORT || 8282),
  basePath: environment.AOW_BASE_PATH || '',
  socket: environment.AOW_TERMINALD_SOCKET || (environment.XDG_RUNTIME_DIR
    ? join(environment.XDG_RUNTIME_DIR, 'aow-terminald/terminald.sock')
    : `/tmp/aow-terminald-${process.getuid()}/terminald.sock`),
}), { mode: 0o600 });
