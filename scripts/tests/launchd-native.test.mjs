// Opt-in integration test: only creates uniquely named jobs and an isolated HOME.
// AOW_NATIVE_SERVICE_TESTS=1 node --test scripts/tests/launchd-native.test.mjs
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import test from 'node:test';
import { waitForHealth } from '../service-health.mjs';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const enabled = process.platform === 'darwin' && process.env.AOW_NATIVE_SERVICE_TESTS === '1';

test('real launchd activates isolated native services, restarts crashes, and rolls back unhealthy upgrades', {
  skip: !enabled, timeout: 90000,
}, async t => {
  const directory = mkdtempSync('/private/tmp/aow-launchd-');
  const home = join(directory, 'home');
  const runtime = join(home, '.local/lib/aow');
  const state = join(home, 'state');
  const socket = join(directory, 'pty/daemon.sock');
  const namespace = `org.aow.test.${process.pid}.${directory.split('-').at(-1).toLowerCase()}`;
  const domain = `gui/${process.getuid()}`;
  const services = ['server', 'terminald'].map(component => `${domain}/${namespace}.${component}`);
  t.after(() => {
    for (const service of services) spawnSync('/bin/launchctl', ['bootout', service], { timeout: 15000 });
    rmSync(directory, { recursive: true, force: true });
  });
  execFileSync('/bin/launchctl', ['print', domain], { stdio: 'ignore' });
  for (const path of ['scripts', 'packaging/bin', 'home/state', 'home/.config/aow']) {
    mkdirSync(join(directory, path), { recursive: true });
  }
  mkdirSync(join(runtime, 'bin'), { recursive: true });
  for (const file of ['start-launchd.sh', 'launchd-service.mjs', 'service-health.mjs', 'server-state-dir.sh']) {
    writeFileSync(join(directory, 'scripts', file), readFileSync(join(root, 'scripts', file), 'utf8')
      .replaceAll('org.aow.', `${namespace}.`), { mode: 0o700 });
  }
  for (const file of ['aow', 'aow-server', 'aow-terminald']) {
    copyFileSync(join(root, 'packaging/bin', file), join(directory, 'packaging/bin', file));
    copyFileSync(join(root, 'packaging/bin', file), join(runtime, 'bin', file));
  }
  const listener = createServer();
  await new Promise(resolve => listener.listen(0, '127.0.0.1', resolve));
  const port = listener.address().port;
  await new Promise(resolve => listener.close(resolve));
  const env = { ...process.env, HOME: home, AOW_RUNTIME_ROOT: runtime,
    AOW_SERVER_STATE_DIR: state, AOW_STATE_DIR: state,
    AOW_SERVER_HOST: '127.0.0.1', AOW_SERVER_PORT: String(port),
    AOW_TERMINALD_SOCKET: socket, XDG_CONFIG_HOME: join(home, '.config'),
    XDG_STATE_HOME: '', XDG_RUNTIME_DIR: '', AOW_LOG_MODE: '', AOW_BASE_PATH: '' };
  writeFileSync(join(home, '.config/aow/server.env'), 'AOW_BASE_PATH=/tools/aow/\n');
  execFileSync(join(root, 'target/debug/aow-server'), ['--initialize-state', '--state-dir', state], { env, stdio: 'pipe' });
  writeFileSync(join(state, 'pin.md5'), 'e10adc3949ba59abbe56e057f20f883e\n', { mode: 0o600 });
  for (const version of ['1.0.0', '2.0.0']) {
    const release = join(runtime, 'releases', version);
    mkdirSync(join(release, 'bin'), { recursive: true });
    mkdirSync(join(release, 'frontend/dist'), { recursive: true });
    writeFileSync(join(release, 'frontend/dist/index.html'), '<html>isolated test</html>');
    for (const component of ['server', 'terminald']) {
      if (version === '1.0.0') copyFileSync(join(root, `target/debug/aow-${component}`), join(release, `bin/aow-${component}`));
      else writeFileSync(join(release, `bin/aow-${component}`), '#!/bin/sh\nexit 1\n', { mode: 0o700 });
    }
  }
  function pid(component) {
    try {
      const status = execFileSync('/bin/launchctl', ['print', `${domain}/${namespace}.${component}`], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
      return /^\s*state = running\s*$/m.test(status) ? Number(status.match(/^\s*pid = (\d+)\s*$/m)?.[1]) : null;
    } catch { return null; }
  }
  function activate(component, version) {
    return spawnSync('/bin/sh', [join(directory, 'scripts/start-launchd.sh'), component, version], {
      env, encoding: 'utf8', timeout: 25000,
    });
  }
  for (const component of ['server', 'terminald']) {
    const config = { component, host: '127.0.0.1', port, socket, basePath: '/tools/aow/' };
    const first = activate(component, '1.0.0');
    assert.ifError(first.error);
    assert.equal(first.status, 0, first.stdout + first.stderr);
    await waitForHealth(config, () => pid(component));
    // Reinstall while the old job is still running. bootout can return before
    // launchd has removed it, so immediate bootstrap used to fail with EIO.
    for (let attempt = 0; attempt < 2; attempt++) {
      const previousPid = pid(component);
      const reinstalled = activate(component, '1.0.0');
      assert.ifError(reinstalled.error);
      assert.equal(reinstalled.status, 0, reinstalled.stdout + reinstalled.stderr);
      await waitForHealth(config, () => pid(component));
      assert.notEqual(pid(component), previousPid);
    }
    const original = pid(component);
    process.kill(original, 'SIGKILL');
    const deadline = Date.now() + 10000;
    while ((!pid(component) || pid(component) === original) && Date.now() < deadline) await delay(100);
    assert.ok(pid(component) && pid(component) !== original, 'KeepAlive must restart an abnormal exit');
    await waitForHealth(config, () => pid(component));
    const upgrade = activate(component, '2.0.0');
    assert.ifError(upgrade.error);
    assert.notEqual(upgrade.status, 0);
    assert.match(upgrade.stderr, /activation failed/);
    assert.equal(realpathSync(join(runtime, 'active', component)), join(runtime, 'releases/1.0.0'));
    await waitForHealth(config, () => pid(component));
    const plist = readFileSync(join(home, 'Library/LaunchAgents', `${namespace}.${component}.plist`), 'utf8');
    assert.match(plist, /AOW_LOG_MODE<\/key><string>unified/);
    assert.equal(existsSync(join(home, 'Library/Logs/AOW')), false);
  }
});
