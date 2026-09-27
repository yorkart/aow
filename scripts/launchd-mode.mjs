import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { userInfo } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const daemonDirectory = '/Library/LaunchDaemons';
const daemonOwner = 0;
export const daemonLabel = component => `org.aow.service.${component}`;

export function launchctl(args) {
  const result = spawnSync('launchctl', args, { encoding: 'utf8', timeout: 5000 });
  if (result.error) throw result.error;
  return result;
}

export function missingDomain(result) {
  // Permission errors and unavailable launchctl are not evidence of no GUI.
  return (result.status === 112 && /could not find domain/i.test(result.stderr))
    || (result.status === 113 && /could not find service/i.test(result.stderr));
}

export function hasGui(uid) {
  const result = launchctl(['print', `gui/${uid}`]);
  if (result.status === 0) return true;
  if (missingDomain(result)) return false;
  throw new Error(`Cannot inspect graphical user session: ${result.stderr.trim() || `launchctl exited ${result.status}`}`);
}

export function readDaemon(component) {
  const path = join(daemonDirectory, `${daemonLabel(component)}.plist`);
  let info;
  try { info = lstatSync(path); } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  if (!info.isFile() || info.uid !== daemonOwner || (info.mode & 0o022)) {
    throw new Error(`Untrusted LaunchDaemon configuration: ${path}`);
  }
  const result = spawnSync('plutil', ['-convert', 'json', '-o', '-', path], { encoding: 'utf8', timeout: 5000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Cannot read LaunchDaemon configuration: ${path}`);
  return JSON.parse(result.stdout);
}

export function resolveService(runtime) {
  const identity = userInfo();
  const home = process.env.HOME;
  if (!home?.startsWith('/') || resolve(runtime) !== resolve(home, '.local/lib/aow')) {
    throw new Error('macOS installation must use the target account HOME/.local/lib/aow');
  }
  let saved;
  try { saved = JSON.parse(readFileSync(join(runtime, 'update.json'), 'utf8')).service; }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (saved != null && (!['launchagent', 'launchdaemon'].includes(saved.mode)
    || saved.user !== identity.username || (saved.uid != null && saved.uid !== identity.uid))) {
    throw new Error('Saved service mode/account does not match the installation account');
  }
  const agents = ['server', 'terminald'].some(component => existsSync(join(home, 'Library/LaunchAgents', `org.aow.${component}.plist`)));
  const daemons = ['server', 'terminald'].map(readDaemon).filter(Boolean);
  const ownedDaemons = daemons.filter(config => config.UserName === identity.username);
  for (const config of ownedDaemons) {
    const component = config.Label?.split('.').at(-1);
    if (!['server', 'terminald'].includes(component) || config.Label !== daemonLabel(component)
      || config.EnvironmentVariables?.HOME !== home || config.EnvironmentVariables?.AOW_RUNTIME_ROOT !== runtime
      || JSON.stringify(config.ProgramArguments) !== JSON.stringify([join(runtime, 'bin', `aow-${component}`)])) {
      throw new Error('Existing LaunchDaemon does not match this installation; migrate it explicitly');
    }
  }
  if (agents && ownedDaemons.length) throw new Error('Both LaunchAgent and LaunchDaemon registrations exist; migrate explicitly');
  if ((saved?.mode === 'launchagent' && ownedDaemons.length) || (saved?.mode === 'launchdaemon' && agents)) {
    throw new Error('Saved service mode conflicts with the installed launchd configuration');
  }
  let mode = saved?.mode;
  if (!mode) mode = ownedDaemons.length ? 'launchdaemon' : agents ? 'launchagent' : hasGui(identity.uid) ? 'launchagent' : 'launchdaemon';
  if (mode === 'launchagent' && !hasGui(identity.uid)) {
    throw new Error('This installation uses LaunchAgent and requires a logged-in graphical user session; it will not silently switch modes');
  }
  if (mode === 'launchdaemon') {
    if (identity.uid === 0) throw new Error('Run the installer as the non-root service account, not with sudo');
    if (daemons.some(config => config.UserName !== identity.username)) throw new Error('AoW LaunchDaemons belong to another account');
  }
  return { mode, user: identity.username, uid: identity.uid };
}

export function registrationHint(runtime) {
  const identity = userInfo();
  console.error(`LaunchDaemon registration is pending: ${join(runtime, 'pending-launchdaemon.json')}`);
  console.error('From an administrator’s local terminal, use an administrator-owned, trusted checkout of AoW:');
  const quotedUser = `'${identity.username.replaceAll("'", "'\\''")}'`;
  console.error(`  sudo -k /usr/bin/python3 -I scripts/register-launchdaemon.py --user ${quotedUser}`);
  console.error('Then rerun just install (or aow update) as the service account. Do not give that account sudo access.');
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, runtime, destination] = process.argv.slice(2);
    if (command === 'hint') registrationHint(runtime);
    else if (command === 'resolve') {
      const service = resolveService(runtime);
      if (destination) writeFileSync(destination, JSON.stringify(service) + '\n', { mode: 0o600 });
      process.stdout.write(service.mode + '\n');
    } else throw new Error('Usage: launchd-mode.mjs resolve|hint RUNTIME [OUTPUT]');
  } catch (error) { console.error(`error: ${error.message}`); process.exitCode = 1; }
}
