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
  // A background-only account can have a user domain but no GUI domain (125).
  // Scope this to the GUI probe; it does not mean a system service is missing.
  if (result.status === 125
    && /could not print domain: 125: domain does not support specified action/i.test(result.stderr)) return false;
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
  console.error('\nInstallation is incomplete: LaunchDaemon registration is pending. To continue:');
  console.error('1. Switch to an administrator account on this Mac and open a local terminal (not SSH).');
  console.error('   Change to a trusted AoW checkout with a local package, owned by the administrator and not writable by the service account.');
  console.error('2. Run the following command and enter the administrator account password when prompted:');
  const quotedUser = `'${identity.username.replaceAll("'", "'\\''")}'`;
  console.error(`  just install --user ${quotedUser}`);
  console.error('   The installer will prepare files as the target user, register services, and verify startup automatically.');
  console.error(`3. After installation succeeds, future updates can run as ${identity.username} with just install or aow update.`);
  console.error('Do not give the service account sudo access.');
  console.error(`Registration request: ${join(runtime, 'pending-launchdaemon.json')}`);
  console.error('Exit code 78 means administrator registration is required and startup has not been verified; just reports this incomplete installation as failed.');
}

export function registrationRequired(service) {
  if (service.mode !== 'launchdaemon') return false;
  if (!readDaemon('server')) return true;
  const result = launchctl(['print', `system/${daemonLabel('server')}`]);
  if (result.status === 0) return false;
  if (missingDomain(result)) return true;
  throw new Error(`Cannot inspect LaunchDaemon registration: ${result.stderr.trim()}`);
}

function firstInstallHint(service) {
  const quotedUser = `'${service.user.replaceAll("'", "'\\''")}'`;
  console.error(`First-time LaunchDaemon setup for ${service.user} requires an administrator account.`);
  console.error('From an administrator-owned, trusted AoW checkout on this Mac, run:');
  console.error(`  just install --user ${quotedUser}`);
  console.error('The administrator installer will prepare files as the target user, register services, and verify startup.');
  console.error('After registration, run updates as the target account without sudo.');
  console.error('Exit code 78 means administrator setup is required; installation is incomplete.');
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [command, runtime, destination] = process.argv.slice(2);
    if (command === 'hint') registrationHint(runtime);
    else if (command === 'check-install') {
      const service = resolveService(runtime);
      process.stdout.write(JSON.stringify({ ...service, registrationRequired: registrationRequired(service) }) + '\n');
    } else if (command === 'resolve') {
      const service = resolveService(runtime);
      if (destination && process.env.AOW_INSTALL_MANAGED !== '1' && registrationRequired(service)) {
        firstInstallHint(service);
        process.exitCode = 78;
      } else {
        if (destination) writeFileSync(destination, JSON.stringify(service) + '\n', { mode: 0o600 });
        process.stdout.write(service.mode + '\n');
      }
    } else throw new Error('Usage: launchd-mode.mjs resolve|check-install|hint RUNTIME [OUTPUT]');
  } catch (error) { console.error(`error: ${error.message}`); process.exitCode = 1; }
}
