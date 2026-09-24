// Opt-in: writes uniquely marked test messages and queries only the test PIDs.
// AOW_NATIVE_LOG_TESTS=1 node --test scripts/tests/unified-logging.test.mjs
import assert from 'node:assert/strict';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import test from 'node:test';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const enabled = process.platform === 'darwin' && process.env.AOW_NATIVE_LOG_TESTS === '1';
const env = { ...process.env, AOW_LOG_MODE: 'unified', RUST_LOG: 'info' };

async function records(pid, check) {
  const deadline = Date.now() + 15000;
  let entries = [];
  while (Date.now() < deadline) {
    entries = JSON.parse(execFileSync('/usr/bin/log', ['show', '--last', '2m', '--style', 'json',
      '--predicate', `subsystem == "org.aow" AND processIdentifier == ${pid}`], {
      encoding: 'utf8', timeout: 10000, maxBuffer: 4 * 1024 * 1024,
    }));
    if (check(entries)) return entries;
    await delay(300);
  }
  assert.fail(`Missing Unified Logging records for PID ${pid}: ${JSON.stringify(entries)}`);
}

test('native Unified Logging preserves severity, fields, Unicode, long errors and panics', {
  skip: !enabled, timeout: 60000,
}, async () => {
  const build = execFileSync('cargo', ['test', '--offline', '--no-run', '-p', 'aow-macos-log', '--message-format=json'], {
    cwd: root, encoding: 'utf8', timeout: 45000,
  });
  const artifact = build.trim().split('\n').map(line => JSON.parse(line))
    .find(message => message.reason === 'compiler-artifact' && message.profile.test && message.executable);
  assert.ok(artifact?.executable);
  const marker = `aow-native-${process.pid}-${Date.now()}`;
  const result = spawnSync(artifact.executable, ['--ignored', '--exact', 'tests::native_log_fixture', '--nocapture'], {
    env: { ...env, AOW_LOG_TEST_MARKER: marker }, encoding: 'utf8', timeout: 10000,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stdout + result.stderr);
  const entries = await records(result.pid, entries => entries.some(entry => entry.eventMessage?.includes(`panic marker ${marker}`)));
  function event(text) {
    const entry = entries.find(entry => entry.eventMessage?.includes(text));
    assert.ok(entry, `Missing message: ${text}`);
    assert.equal(entry.category, 'test');
    return entry;
  }
  assert.equal(event(`info marker ${marker}`).messageType, 'Default');
  assert.equal(event(`warn marker ${marker}`).messageType, 'Default');
  assert.equal(event(`error marker ${marker}`).messageType, 'Error');
  assert.equal(event(`fatal marker ${marker}`).messageType, 'Error');
  assert.equal(event(`panic marker ${marker}`).messageType, 'Fault');
  assert.match(event(`info marker ${marker}`).eventMessage, /fixture\{run=/);
  event(`UTF-8 中文 🦀 NUL:\\0 END ${marker}`);
  event(`tail ${marker}`);
  assert.doesNotMatch(entries.map(entry => entry.eventMessage).join('\n'), /\x1b\[|�/);
});

for (const component of ['server', 'terminald']) {
  test(`${component} startup errors survive RUST_LOG=off and foreground errors stay on stderr`, {
    skip: !enabled, timeout: 25000,
  }, async () => {
    const marker = `--invalid-${process.pid}-${Date.now()}`;
    const binary = join(root, `target/debug/aow-${component}`);
    const result = spawnSync(binary, [marker], { env: { ...env, RUST_LOG: 'off' }, encoding: 'utf8', timeout: 5000 });
    assert.ifError(result.error);
    assert.notEqual(result.status, 0);
    const entries = await records(result.pid, entries => entries.some(entry => entry.eventMessage?.includes(marker)));
    const entry = entries.find(entry => entry.eventMessage?.includes(marker));
    assert.equal(entry.category, component);
    assert.equal(entry.messageType, 'Error');
    const foreground = spawnSync(binary, [marker], { env: { ...env, AOW_LOG_MODE: '' }, encoding: 'utf8', timeout: 5000 });
    assert.notEqual(foreground.status, 0);
    assert.ok(foreground.stderr.includes(marker));
    const help = spawnSync(binary, ['--help'], { env: { ...env, AOW_LOG_MODE: '' }, encoding: 'utf8', timeout: 5000 });
    assert.equal(help.status, 0);
    assert.match(help.stdout, /Usage: aow-/);
  });
}

test('macOS terminald forwards Node worker stderr into Unified Logging', {
  skip: !enabled, timeout: 30000,
}, async t => {
  const directory = mkdtempSync('/private/tmp/aow-log-');
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const marker = `vt-worker-${process.pid}-${Date.now()}`;
  const worker = join(directory, 'broken-worker.mjs');
  writeFileSync(worker, `process.stderr.write(${JSON.stringify(`${marker}\n`)}); setTimeout(() => process.exit(1), 200);\n`);
  const child = spawn(join(root, 'target/debug/aow-terminald'), ['--socket', join(directory, 'terminald.sock'),
    '--vt-node', process.execPath, '--vt-worker', worker], { env, stdio: 'ignore' });
  const exited = new Promise(resolve => child.once('exit', resolve));
  t.after(async () => { child.kill('SIGTERM'); await exited; });
  const entries = await records(child.pid, entries => entries.some(entry => entry.eventMessage?.includes(marker)));
  const entry = entries.find(entry => entry.eventMessage?.includes(marker));
  assert.equal(entry.category, 'terminald');
  assert.match(entry.eventMessage, /VT worker stderr/);
});
