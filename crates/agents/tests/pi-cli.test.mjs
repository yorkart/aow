import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test } from 'node:test';
import bridge from '../src/pi_bridge/extension.mjs';

test('Pi bridge binds exact native sessions, follows switches, and records only settled completion', () => {
  const directory = mkdtempSync(join(tmpdir(), 'aow-pi-bridge-'));
  const previous = process.env.AOW_PI_BINDING;
  process.env.AOW_PI_BINDING = join(directory, 'binding.json');
  try {
    const handlers = new Map();
    const entries = [];
    bridge({ on: (event, callback) => handlers.set(event, callback), appendEntry: (type, data) => entries.push({ type, data }) });
    let id = 'first';
    const context = { sessionManager: { getCwd: () => directory, getSessionId: () => id, getSessionFile: () => join(directory, `${id}.jsonl`) } };
    handlers.get('session_start')({}, context);
    const binding = () => JSON.parse(readFileSync(process.env.AOW_PI_BINDING, 'utf8'));
    assert.equal(binding().pid, process.pid);
    assert.equal(binding().session_id, 'first');
    assert.equal(handlers.has('agent_end'), false);
    handlers.get('agent_settled')({}, context);
    assert.equal(entries.at(-1).data.event, 'settled');
    handlers.get('session_shutdown')();
    id = 'second';
    handlers.get('session_start')({}, context);
    assert.equal(binding().session_id, 'second');
    handlers.get('session_tree')({}, context);
    assert.equal(entries.at(-1).data.event, 'branch');
    handlers.get('session_shutdown')();
    handlers.get('session_shutdown')();
    assert.deepEqual(readdirSync(directory), []);
  } finally {
    if (previous === undefined) delete process.env.AOW_PI_BINDING;
    else process.env.AOW_PI_BINDING = previous;
    rmSync(directory, { recursive: true, force: true });
  }
});

test('installed Pi persists, resumes and propagates failures with an offline provider', { skip: !process.env.AOW_PI_TEST_CLI }, () => {
  const directory = mkdtempSync(join(tmpdir(), 'aow-pi-native-'));
  const provider = fileURLToPath(new URL('./fixtures/pi-provider.mjs', import.meta.url));
  const extension = fileURLToPath(new URL('../src/pi_bridge/extension.mjs', import.meta.url));
  const env = { ...process.env, PI_CODING_AGENT_DIR: directory, PI_CODING_AGENT_SESSION_DIR: join(directory, 'sessions'),
    PI_OFFLINE: '1', PI_SKIP_VERSION_CHECK: '1', AOW_PI_BINDING: join(directory, 'binding.json') };
  const common = ['--offline', '--no-extensions', '--no-skills', '--no-prompt-templates', '--no-context-files', '--no-approve',
    '--extension', provider, '--extension', extension, '--model', 'aow-test/fixture', '--print'];
  const run = (args, extra = {}) => spawnSync(process.env.AOW_PI_TEST_CLI, [...common, ...args], {
    cwd: directory, env: { ...env, ...extra }, input: 'Hello from AoW', encoding: 'utf8', timeout: 20000,
  });
  try {
    const first = run(['--session-id', 'aow-pi-native']);
    assert.equal(first.status, 0, first.stderr);
    assert.match(first.stdout, /Pi fixture reply/);
    const path = join(directory, 'sessions', readdirSync(join(directory, 'sessions'))[0]);
    const records = () => readFileSync(path, 'utf8').trim().split('\n').map(line => JSON.parse(line));
    assert.equal(records()[0].id, 'aow-pi-native');
    assert.equal(records().at(-1).customType, 'aow.pi');
    assert.equal(records().at(-1).data.event, 'settled');
    const resumed = run(['--session', 'aow-pi-native']);
    assert.equal(resumed.status, 0, resumed.stderr);
    assert.equal(readdirSync(join(directory, 'sessions')).length, 1);
    assert.equal(records().filter(entry => entry.message?.role === 'user').length, 2);
    const failed = run(['--session-id', 'aow-pi-failed'], { AOW_PI_TEST_FAILURE: '1' });
    assert.equal(failed.status, 1, failed.stderr);
    assert.match(failed.stderr, /fixture failure/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
