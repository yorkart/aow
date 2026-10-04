import { execFileSync, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync,
  realpathSync, renameSync, rmSync, rmdirSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { userInfo } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { isDeepStrictEqual } from 'node:util';
import { daemonLabel, launchctl, missingDomain, readDaemon, resolveService } from './launchd-mode.mjs';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const [component, runtime, requested = 'latest'] = process.argv.slice(2);
let stage, lock;

function job() {
  const result = launchctl(['print', `system/${daemonLabel(component)}`]);
  if (result.status !== 0) {
    if (missingDomain(result)) return { registered: false, pid: null };
    throw new Error(`Cannot inspect LaunchDaemon: ${result.stderr.trim()}`);
  }
  const pid = Number(result.stdout.match(/^\s*pid = (\d+)\s*$/m)?.[1]);
  return { registered: true, pid: Number.isSafeInteger(pid) && pid > 1 ? pid : null };
}

function signalOwnJob(signal = 'TERM', expectedPid = null) {
  const { pid } = job();
  if (!pid || (expectedPid !== null && pid !== expectedPid)) return null;
  const owner = spawnSync('ps', ['-o', 'uid=', '-p', String(pid)], { encoding: 'utf8', timeout: 5000 });
  if (owner.error) throw owner.error;
  if (owner.status === 1 && !owner.stdout.trim()) return pid; // Already exited.
  if (owner.status !== 0 || Number(owner.stdout.trim()) !== process.getuid()) {
    throw new Error('Refusing to signal a LaunchDaemon owned by a different account');
  }
  if (job().pid !== pid) return pid;
  // The kernel also enforces UID ownership. Never invoke sudo or signal a group.
  const result = spawnSync('kill', [`-${signal}`, String(pid)], { encoding: 'utf8', timeout: 5000 });
  if (result.error) throw result.error;
  if (result.status !== 0 && job().pid === pid) throw new Error(`Cannot restart LaunchDaemon: ${result.stderr.trim()}`);
  return pid;
}

async function restartOwnJob() {
  const previousPid = signalOwnJob();
  if (!previousPid) return null;
  // A direct SIGTERM does not use launchd's ExitTimeOut. Long-lived HTTP
  // connections can keep graceful shutdown waiting after the listener closes.
  const deadline = Date.now() + 5000;
  while (job().pid === previousPid) {
    if (Date.now() >= deadline) {
      console.error(`aow-${component} did not exit within 5 seconds; forcing the previous process to stop.`);
      // Recheck both the job PID and its UID; never kill a replacement process.
      signalOwnJob('KILL', previousPid);
      break;
    }
    await delay(100);
  }
  return previousPid;
}

function linkSnapshot(path) {
  let info;
  try { info = lstatSync(path); } catch (error) { if (error.code === 'ENOENT') return null; throw error; }
  if (!info.isSymbolicLink()) throw new Error(`Refusing to replace unmanaged entry: ${path}`);
  return readlinkSync(path);
}

function replaceLink(target, path) {
  if (target === null) { if (existsSync(path) || linkSnapshot(path) !== null) unlinkSync(path); return; }
  const pending = join(stage, randomUUID());
  symlinkSync(target, pending);
  renameSync(pending, path);
}

function writeAtomic(path, bytes) {
  const pending = join(stage, randomUUID());
  writeFileSync(pending, bytes, { mode: 0o600 });
  renameSync(pending, path);
}

function requestDocument() {
  const identity = userInfo();
  const path = join(runtime, 'pending-launchdaemon.json');
  let document;
  try {
    const info = lstatSync(path);
    if (!info.isFile() || info.uid !== identity.uid) throw new Error('Untrusted pending LaunchDaemon request');
    document = JSON.parse(readFileSync(path, 'utf8'));
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  document ||= { version: 1, user: identity.username, uid: identity.uid,
    home: process.env.HOME, runtime, components: {} };
  if (document.version !== 1 || document.user !== identity.username || document.uid !== identity.uid
    || document.home !== process.env.HOME || document.runtime !== runtime
    || !document.components || Array.isArray(document.components) || typeof document.components !== 'object') {
    throw new Error('Pending LaunchDaemon request belongs to another installation');
  }
  return { path, document };
}

function clearRequest() {
  const { path, document } = requestDocument();
  delete document.components[component];
  if (Object.keys(document.components).length) writeAtomic(path, JSON.stringify(document, null, 2) + '\n');
  else if (existsSync(path)) unlinkSync(path);
}

function waitForHealth(previousPid) {
  // Existing helper checks both the endpoint's PID and launchd's running PID.
  execFileSync('node', [join(scriptDir, 'service-health.mjs'), join(stage, 'health.json'),
    `system/${daemonLabel(component)}`, ...(previousPid ? [String(previousPid)] : [])],
  { stdio: ['ignore', 'pipe', 'pipe'], timeout: 35000 });
}

async function main() {
  if (!['server', 'terminald'].includes(component) || !runtime?.startsWith('/')) throw new Error('Invalid LaunchDaemon activation arguments');
  if (resolveService(runtime).mode !== 'launchdaemon') throw new Error('Installation is not configured for LaunchDaemon');
  if (requested !== 'latest' && !/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(requested)) throw new Error('Invalid release id');
  const releases = realpathSync(join(runtime, 'releases'));
  const release = realpathSync(join(runtime, requested === 'latest' ? 'latest' : `releases/${requested}`));
  if (!release.startsWith(releases + '/') || !existsSync(join(release, 'bin', `aow-${component}`))
    || !existsSync(join(runtime, 'bin', `aow-${component}`))) throw new Error('Incomplete or invalid AoW release');
  if (component === 'server' && !existsSync(join(release, 'frontend/dist/index.html'))) throw new Error('Missing frontend');
  const lockPath = join(runtime, '.launchdaemon-activation.lock');
  mkdirSync(lockPath, { mode: 0o700 });
  lock = lockPath;
  stage = mkdtempSync(join(runtime, '.launchdaemon-'));
  mkdirSync(join(runtime, 'active'), { recursive: true, mode: 0o700 });
  const active = join(runtime, 'active', component);
  const previous = linkSnapshot(active);
  const nodeLink = join(runtime, 'node');
  const nodeMarker = join(runtime, '.aow-terminald-node-target');
  const previousNode = component === 'terminald' ? linkSnapshot(nodeLink) : null;
  const previousMarker = component === 'terminald' && existsSync(nodeMarker) ? readFileSync(nodeMarker) : null;
  // A daemon terminal inherits runtime/node in PATH. Resolve that managed link
  // before replacing it, otherwise an update can create a self-referencing link.
  const nodePath = realpathSync(execFileSync('/bin/sh', ['-c', 'command -v node'], { encoding: 'utf8' }).trim());
  const plist = join(stage, 'service.plist');
  execFileSync('node', [join(scriptDir, 'launchd-service.mjs'), component, runtime, plist, nodePath, 'launchdaemon'], { stdio: 'pipe' });
  const desired = JSON.parse(execFileSync('plutil', ['-convert', 'json', '-o', '-', plist], { encoding: 'utf8' }));
  const installed = readDaemon(component);
  const status = job();
  if (!isDeepStrictEqual(installed, desired) || !status.registered) {
    const { path, document } = requestDocument();
    document.components[component] = desired;
    writeAtomic(path, JSON.stringify(document, null, 2) + '\n');
    // Seed a first installation so the administrator can start it. Existing
    // releases keep running until registration succeeds and install is retried.
    if (previous === null) {
      if (component === 'terminald') {
        replaceLink(nodePath, nodeLink);
        writeAtomic(nodeMarker, nodePath + '\n');
      }
      replaceLink(release, active);
    }
    console.error(`Prepared ${component}; local administrator registration is required before activation.`);
    return 78;
  }
  try {
    if (component === 'terminald') {
      replaceLink(nodePath, nodeLink);
      writeAtomic(nodeMarker, nodePath + '\n');
    }
    replaceLink(release, active);
    waitForHealth(await restartOwnJob());
  } catch (error) {
    replaceLink(previous, active);
    if (component === 'terminald') {
      replaceLink(previousNode, nodeLink);
      if (previousMarker !== null) writeAtomic(nodeMarker, previousMarker);
      else if (existsSync(nodeMarker)) unlinkSync(nodeMarker);
    }
    let restored = false;
    try {
      const failedPid = await restartOwnJob();
      if (previous !== null) { waitForHealth(failedPid); restored = true; }
    } catch { console.error(`error: failed to restore previous ${component} LaunchDaemon; inspect system/${daemonLabel(component)}`); }
    throw new Error(`aow-${component} activation failed; ${restored ? 'restored previous release' : 'restored previous release link, but service recovery was not verified'} (${error.message})`, { cause: error });
  }
  clearRequest();
  console.log(`aow-${component} is active on release ${release.split('/').at(-1)} (LaunchDaemon)`);
  return 0;
}

try { process.exitCode = await main(); }
catch (error) { console.error(`error: ${error.message}`); process.exitCode = 1; }
finally {
  if (stage) rmSync(stage, { recursive: true, force: true });
  if (lock) rmdirSync(lock);
}
