import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { access, mkdir, mkdtemp, readFile, rm, stat, unlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { setAccount } from '../dist/auth.js';
import { configuration } from '../dist/config.js';
import { openDatabase } from '../dist/database.js';
import { createOrbitServer } from '../dist/http.js';
import { Connectors } from '../dist/connectors/service.js';

const local = { name: 'Local Codex', provider: 'openai', executor: 'codex', auth_method: 'local', account_note: '' };
const api = { name: 'API connection', provider: 'openai', executor: 'codex', auth_method: 'api_key', account_note: 'Project identity' };
const errorCode = code => error => error.code === code;

async function fixture(t, extra = {}) {
  const directory = await mkdtemp(join(tmpdir(), 'orbit-connectors-'));
  const home = join(directory, 'operator'); const bin = join(directory, 'bin'); const state = join(directory, 'state');
  for (const path of [bin, join(home, '.codex'), join(home, '.claude'), join(home, '.gemini')]) await mkdir(path, { recursive: true });
  const script = await readFile(new URL('./fixtures/connector-cli.mjs', import.meta.url), 'utf8');
  for (const name of ['codex', 'claude', 'gemini']) await writeFile(join(bin, name), `#!${process.execPath}\n${script}`, { mode: 0o700 });
  await writeFile(join(home, '.codex/auth.json'), JSON.stringify({ OPENAI_API_KEY: 'operator-first-identity' }));
  await writeFile(join(home, '.claude/.credentials.json'), JSON.stringify({ claudeAiOauth: { accessToken: 'fixture-access', refreshToken: 'fixture-refresh' } }));
  await writeFile(join(home, '.claude.json'), JSON.stringify({ oauthAccount: { emailAddress: 'operator@example.com' }, customInstructions: 'must-not-be-copied' }));
  await writeFile(join(home, '.gemini/oauth_creds.json'), JSON.stringify({ access_token: 'gemini-fixture', refresh_token: 'gemini-refresh' }));
  const calls = [];
  const options = {
    localHome: home,
    env: { ...process.env, PATH: bin + delimiter + process.env.PATH, CODEX_HOME: join(home, '.codex'), CLAUDE_CONFIG_DIR: join(home, '.claude'), GEMINI_CLI_HOME: home, OPENAI_API_KEY: 'operator-env-key-must-not-leak', ...extra.env },
    fetch: extra.fetch ?? (async (url, init) => {
      const headers = new Headers(init.headers); calls.push({ url: String(url), headers });
      const key = headers.get('authorization') ?? headers.get('x-api-key');
      return key?.includes('invalid') ? new Response(JSON.stringify({ error: `reflected ${key}` }), { status: 401 }) : Response.json({ data: [{ id: 'fixture-model' }] });
    }),
  };
  let database = openDatabase(state);
  const org = randomUUID(); const other = randomUUID();
  for (const id of [org, other]) database.prepare('INSERT INTO orgs VALUES (?, ?, ?)').run(id, id, new Date().toISOString());
  let connectors = new Connectors(database, state, options);
  t.after(async () => { await connectors.close(); database.close(); await rm(directory, { recursive: true, force: true }); });
  return { org, other, home, bin, state, options, calls, get database() { return database; }, get connectors() { return connectors; }, async restart() { await connectors.close(); database.close(); database = openDatabase(state); connectors = new Connectors(database, state, options); } };
}

async function statusUntil(app, id, expected) {
  for (let i = 0; i < 160; i++) {
    const status = app.connectors.authFlows.publicStatus(app.org, id);
    if (status.status === expected) return status;
    if (['error', 'expired', 'cancelled'].includes(status.status)) throw new Error(`Unexpected auth outcome: ${status.status}: ${status.message}`);
    await delay(25);
  }
  throw new Error(`Authentication did not reach ${expected}.`);
}

test('connector CRUD is org-scoped, keys are encrypted and redacted, and stale edits and referenced deletion are rejected', async t => {
  const app = await fixture(t);
  const pending = await app.connectors.save(app.org, local);
  assert.equal(pending.status, 'auth_required'); assert.equal(pending.credential_configured, false);
  const checked = await app.connectors.check(app.org, pending.id); assert.equal(checked.status, 'available');
  const key = 'private-api-credential-for-fixture';
  const created = await app.connectors.save(app.org, { ...api, api_key: key, connect: true });
  assert.equal(created.status, 'available'); assert.equal(created.credential_configured, true);
  assert.deepEqual(created.models, ['fixture-model']);
  assert.equal(app.calls.at(-1).url, 'https://api.openai.com/v1/models');
  assert.equal(app.calls.at(-1).headers.get('authorization'), `Bearer ${key}`);
  assert.equal(JSON.stringify(created).includes(key), false);
  assert.equal(Object.hasOwn(created, 'credentials'), false);
  assert.equal(JSON.stringify(app.database.prepare('SELECT * FROM connectors WHERE id = ?').get(created.id)).includes(key), false);
  assert.equal((await stat(join(app.state, 'credentials.key'))).mode & 0o777, 0o600);
  assert.throws(() => app.connectors.get(app.other, created.id), errorCode('connector_not_found'));
  assert.deepEqual(app.connectors.list(app.other), []);
  await assert.rejects(app.connectors.save(app.org, { ...api, revision: 999 }, created.id), errorCode('connector_conflict'));
  const edited = await app.connectors.save(app.org, { ...api, name: 'Renamed', revision: created.revision }, created.id);
  assert.equal(edited.credential_configured, true); assert.equal(edited.status, 'available');
  await app.restart(); assert.equal((await app.connectors.check(app.org, created.id)).status, 'available');
  app.connectors.setAgentConnector(app.org, 'agent-A', created.id);
  assert.equal(app.connectors.get(app.org, created.id).agent_count, 1);
  assert.throws(() => app.connectors.remove(app.org, created.id, edited.revision), errorCode('connector_in_use'));
  app.database.prepare('DELETE FROM agent_connectors WHERE connector_id = ?').run(created.id);
  app.connectors.remove(app.org, created.id, edited.revision);
  assert.throws(() => app.connectors.get(app.org, created.id), errorCode('connector_not_found'));
});

test('each local execution reads fresh operator credentials, isolates homes and never copies global instructions or writes back', async t => {
  const app = await fixture(t);
  await writeFile(join(app.home, 'AGENTS.md'), 'global instructions must stay local');
  await writeFile(join(app.home, '.codex/config.toml'), 'developer_instructions = "must-not-be-copied"\n');
  const connector = await app.connectors.save(app.org, { ...local, connect: true });
  assert.equal(app.database.prepare('SELECT credentials FROM connectors WHERE id = ?').get(connector.id).credentials, null);
  const first = await app.connectors.prepareExecution(app.org, connector.id, 'run-one');
  assert.notEqual(first.home, app.home); assert.equal(first.env.HOME, first.home);
  assert.equal(first.env.CODEX_HOME, join(first.home, '.codex')); assert.equal(first.env.OPENAI_API_KEY, undefined);
  await assert.rejects(access(join(first.home, 'AGENTS.md')));
  assert.doesNotMatch(await readFile(join(first.home, '.codex/config.toml'), 'utf8'), /developer_instructions/);
  assert.equal((await stat(join(first.home, '.codex/auth.json'))).mode & 0o777, 0o600);
  assert.equal(JSON.parse(await readFile(join(first.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'operator-first-identity');
  await writeFile(join(first.home, '.codex/auth.json'), JSON.stringify({ OPENAI_API_KEY: 'only-inside-the-run' }));
  assert.equal(JSON.parse(await readFile(join(app.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'operator-first-identity');
  await writeFile(join(app.home, '.codex/auth.json'), JSON.stringify({ OPENAI_API_KEY: 'operator-new-identity' }));
  const second = await app.connectors.prepareExecution(app.org, connector.id, 'run-two');
  assert.equal(JSON.parse(await readFile(join(second.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'operator-new-identity');
  assert.equal(JSON.parse(await readFile(join(first.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'only-inside-the-run');
  await assert.rejects(app.connectors.prepareExecution(app.org, connector.id, 'run-one'), errorCode('execution_exists'));
  const disabled = app.connectors.enable(app.org, connector.id, { revision: connector.revision, enabled: false });
  assert.equal(disabled.status, 'disabled');
  await assert.rejects(app.connectors.prepareExecution(app.org, connector.id, 'new-run'), errorCode('connector_unavailable'));
  await access(join(first.home, '.codex/auth.json'));
  const enabled = app.connectors.enable(app.org, connector.id, { revision: disabled.revision, enabled: true });
  assert.equal(enabled.status, 'auth_required');
  await unlink(join(app.home, '.codex/auth.json'));
  assert.equal((await app.connectors.check(app.org, connector.id)).status, 'auth_required');
});

test('independent Codex device login and Claude browser-code login use real child/PTY flows and single-use encrypted results', async t => {
  const app = await fixture(t);
  const device = await app.connectors.authFlows.start(app.org, 'openai', 'codex');
  const waiting = await statusUntil(app, device.id, 'waiting');
  assert.equal(waiting.user_code, 'ABCD-EFGH');
  assert.equal(waiting.verification_uri, 'https://auth.openai.com/codex/device');
  assert.throws(() => app.connectors.authFlows.publicStatus(app.other, device.id), errorCode('auth_session_not_found'));
  const completed = await statusUntil(app, device.id, 'complete');
  assert.equal(Object.hasOwn(completed, 'credentials'), false);
  const connected = await app.connectors.save(app.org, { ...local, auth_method: 'independent', auth_session_id: device.id, connect: true });
  assert.equal(connected.status, 'available');
  const prepared = await app.connectors.prepareExecution(app.org, connected.id, 'independent-run');
  assert.equal(JSON.parse(await readFile(join(prepared.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'device-flow-fixture-identity');
  await assert.rejects(app.connectors.save(app.org, { ...local, auth_method: 'independent', auth_session_id: device.id }), errorCode('invalid_auth_session'));
  const browser = await app.connectors.authFlows.start(app.org, 'anthropic', 'claude');
  const prompt = await statusUntil(app, browser.id, 'waiting'); assert.match(prompt.verification_uri, /^https:\/\/claude.ai\/oauth\/authorize/);
  await app.connectors.authFlows.submit(app.org, browser.id, 'fixture-code#state');
  await statusUntil(app, browser.id, 'complete');
  const claude = await app.connectors.save(app.org, { name: 'Independent Claude', provider: 'anthropic', executor: 'claude', auth_method: 'independent', auth_session_id: browser.id, connect: true });
  assert.equal(claude.status, 'available');
  const execution = await app.connectors.prepareExecution(app.org, claude.id, 'claude-run');
  assert.match(execution.env.CLAUDE_CODE_OAUTH_TOKEN, /^sk-ant-oat01-/);
  assert.equal(JSON.stringify(app.connectors.get(app.org, claude.id)).includes('sk-ant-oat01-'), false);
  assert.equal(JSON.parse(await readFile(join(app.home, '.codex/auth.json'), 'utf8')).OPENAI_API_KEY, 'operator-first-identity');
});

test('bad keys, missing executors, mismatched auth methods and cancelled authorization never become available', async t => {
  const app = await fixture(t, { env: { ORBIT_FIXTURE_DEVICE_MODE: 'wait' } });
  const invalid = await app.connectors.verify(app.org, { ...api, api_key: 'invalid-private-value' });
  assert.equal(invalid.status, 'auth_required'); assert.equal(JSON.stringify(invalid).includes('invalid-private-value'), false);
  await assert.rejects(app.connectors.save(app.org, { ...api, api_key: 'invalid-private-value', connect: true }), errorCode('connection_failed'));
  assert.deepEqual(app.connectors.list(app.org), []);
  await assert.rejects(app.connectors.save(app.org, { ...local, provider: 'deepseek', auth_method: 'local' }), errorCode('unsupported_connection'));
  await assert.rejects(app.connectors.save(app.org, { ...local, provider: 'made-up-provider' }), errorCode('unsupported_connection'));
  const auth = await app.connectors.authFlows.start(app.org, 'openai', 'codex'); await statusUntil(app, auth.id, 'waiting');
  app.connectors.authFlows.cancel(app.org, auth.id);
  assert.equal(app.connectors.authFlows.publicStatus(app.org, auth.id).status, 'cancelled');
  await assert.rejects(app.connectors.save(app.org, { ...local, auth_method: 'independent', auth_session_id: auth.id }), errorCode('invalid_auth_session'));
  await unlink(join(app.bin, 'claude'));
  const absent = await app.connectors.verify(app.org, { ...local, provider: 'anthropic', executor: 'claude' });
  assert.equal(absent.status, 'error'); assert.match(absent.message, /未安装/);
});

test('parallel authorization admission is bounded and expired results cannot be used', async t => {
  const app = await fixture(t, { env: { ORBIT_FIXTURE_DEVICE_MODE: 'wait' } });
  const attempts = await Promise.allSettled(Array.from({ length: 3 }, () => app.connectors.authFlows.start(app.org, 'openai', 'codex')));
  assert.equal(attempts.filter(item => item.status === 'fulfilled').length, 2);
  assert.equal(attempts.find(item => item.status === 'rejected').reason.code, 'auth_busy');
  for (const attempt of attempts) if (attempt.status === 'fulfilled') app.connectors.authFlows.cancel(app.org, attempt.value.id);
  const expired = app.database.prepare('SELECT id FROM connector_auth_sessions LIMIT 1').get();
  app.database.prepare("UPDATE connector_auth_sessions SET status = 'complete', credentials = 'expired-ciphertext', expires_at = 0 WHERE id = ?").run(expired.id);
  assert.equal(app.connectors.authFlows.publicStatus(app.org, expired.id).status, 'expired');
  assert.equal(app.database.prepare('SELECT credentials FROM connector_auth_sessions WHERE id = ?').get(expired.id).credentials, null);
  await assert.rejects(app.connectors.save(app.org, { ...local, auth_method: 'independent', auth_session_id: expired.id }), errorCode('invalid_auth_session'));
});

test('Claude and Gemini local imports copy only authentication/model data, and DeepSeek keys produce executor-specific settings', async t => {
  const app = await fixture(t);
  const claude = await app.connectors.save(app.org, { name: 'Local Claude', provider: 'anthropic', executor: 'claude', auth_method: 'local', connect: true });
  assert.equal(claude.account_summary, 'operator@example.com');
  const plan = await app.connectors.prepareExecution(app.org, claude.id, 'claude-local');
  assert.equal(plan.env.ANTHROPIC_API_KEY, undefined);
  assert.equal((await readFile(join(plan.home, '.claude.json'), 'utf8')).includes('customInstructions'), false);
  assert.equal(JSON.parse(await readFile(join(plan.home, '.claude/.credentials.json'), 'utf8')).claudeAiOauth.accessToken, 'fixture-access');
  await writeFile(join(app.home, '.gemini/settings.json'), JSON.stringify({ model: { name: 'fixture-gemini-model' }, mcpServers: { private: { command: 'must-not-be-copied' } } }));
  const gemini = await app.connectors.save(app.org, { name: 'Gemini', provider: 'gemini', executor: 'gemini', auth_method: 'local', connect: true });
  const google = await app.connectors.prepareExecution(app.org, gemini.id, 'gemini-local');
  const settings = JSON.parse(await readFile(join(google.home, '.gemini/settings.json'), 'utf8'));
  assert.equal(settings.model.name, 'fixture-gemini-model'); assert.equal(Object.hasOwn(settings, 'mcpServers'), false);
  const deepseek = await app.connectors.save(app.org, { name: 'DeepSeek', provider: 'deepseek', executor: 'claude', auth_method: 'api_key', api_key: 'deepseek-fixture-key', connect: true });
  const deepseekPlan = await app.connectors.prepareExecution(app.org, deepseek.id, 'deepseek-run');
  assert.equal(deepseekPlan.env.ANTHROPIC_AUTH_TOKEN, 'deepseek-fixture-key');
  assert.equal(deepseekPlan.env.ANTHROPIC_BASE_URL, 'https://api.deepseek.com/anthropic');
  assert.equal(deepseekPlan.env.ANTHROPIC_API_KEY, undefined);
  assert.equal(deepseekPlan.env.OPENAI_API_KEY, undefined);
  await unlink(join(app.bin, 'gemini'));
  assert.equal((await app.connectors.adapters.catalog()).providers.some(provider => provider.id === 'gemini'), false);
});

test('Connectors HTTP APIs enforce login, origin and org boundaries without exposing native credentials', async t => {
  const app = await fixture(t);
  await setAccount(app.database, 'connector-user', 'test-only-password');
  const server = createOrbitServer(configuration({ ORBIT_STATE_DIR: app.state }), app.database, app.options);
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  t.after(() => new Promise(resolve => { server.close(resolve); server.closeAllConnections(); }));
  const url = `http://127.0.0.1:${server.address().port}`; let cookie;
  const request = (path, method = 'GET', payload, headers = {}) => fetch(url + path, { method, headers: { ...(cookie ? { Cookie: cookie } : {}), 'Content-Type': 'application/json', ...headers }, ...(payload ? { body: JSON.stringify(payload) } : {}) });
  const root = `/api/orgs/${app.org}/connectors`;
  assert.equal((await request(root)).status, 401);
  const login = await request('/api/auth/login', 'POST', { username: 'connector-user', password: 'test-only-password' }); cookie = login.headers.get('set-cookie').split(';')[0];
  assert.equal((await request(root, 'POST', local, { Origin: 'https://evil.example' })).status, 403);
  const catalog = await (await request(`${root}/catalog`)).json(); assert.ok(catalog.providers.some(item => item.id === 'openai'));
  const created = await request(root, 'POST', { ...local, connect: true }); assert.equal(created.status, 201); const connector = await created.json();
  assert.equal((await request(`/api/orgs/${app.other}/connectors/${connector.id}`)).status, 404);
  assert.equal((await request(`${root}/${connector.id}/enabled`, 'PUT', { revision: connector.revision, enabled: false })).status, 200);
  assert.equal((await request(`${root}/${connector.id}/check`, 'POST')).status, 409);
  const current = await (await request(`${root}/${connector.id}`)).json(); assert.equal(JSON.stringify(current).includes('operator-first-identity'), false);
  assert.equal((await request(`${root}/${connector.id}?revision=${current.revision}`, 'DELETE')).status, 204);
  assert.deepEqual(await (await request(root)).json(), []);
});
