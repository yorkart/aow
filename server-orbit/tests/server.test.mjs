import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { once } from 'node:events';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { setAccount } from '../dist/auth.js';
import { configuration } from '../dist/config.js';
import { openDatabase } from '../dist/database.js';
import { createOrbitServer } from '../dist/http.js';

async function fixture(t, extra = {}) {
  const directory = await mkdtemp(join(tmpdir(), 'orbit-server-'));
  const frontendDir = join(directory, 'frontend');
  await mkdir(frontendDir);
  await writeFile(join(frontendDir, 'index.html'), '<!doctype html><title>Orbit</title>');
  const config = { ...configuration({ ORBIT_STATE_DIR: directory, ORBIT_PORT: '0' }), frontendDir, ...extra };
  let database = openDatabase(directory);
  let server;
  let url;
  const start = async () => {
    server = createOrbitServer(config, database);
    server.listen(0, '127.0.0.1');
    await once(server, 'listening');
    url = `http://127.0.0.1:${server.address().port}`;
  };
  const stop = () => new Promise((resolve, reject) => {
    server.close(error => error ? reject(error) : resolve());
    server.closeAllConnections();
  });
  await start();
  let cookie;
  t.after(async () => { await stop(); database.close(); await rm(directory, { recursive: true, force: true }); });
  const request = (path, { json, method = json ? 'POST' : 'GET', headers = {}, ...options } = {}) => fetch(`${url}${path}`, {
    ...options, method,
    headers: { ...(cookie ? { Cookie: cookie } : {}), ...(json ? { 'Content-Type': 'application/json' } : {}), ...headers },
    ...(json ? { body: JSON.stringify(json) } : {}),
  });
  const login = async (username = 'orbit-user', password = 'test-only-password') => {
    const response = await request('/api/auth/login', { json: { method: 'password', username, password } });
    if (response.ok) cookie = response.headers.get('set-cookie').split(';')[0];
    return response;
  };
  return {
    request, login, directory, get database() { return database; }, get cookie() { return cookie; },
    configure: () => setAccount(database, 'orbit-user', 'test-only-password'),
    restart: async () => { await stop(); database.close(); database = openDatabase(directory); await start(); },
  };
}

test('authentication protects APIs and uses salted AoW-compatible passwords and private cookies', async t => {
  const app = await fixture(t);
  assert.equal((await (await app.request('/api/auth/status')).json()).configured, false);
  assert.equal((await app.request('/api/orgs')).status, 401);
  await app.configure();
  const stored = app.database.prepare('SELECT * FROM account').get();
  assert.equal(stored.salt.length, 32);
  assert.equal(stored.password_hash.length, 64);
  assert.equal((await app.login('orbit-user', 'wrong')).status, 401);
  const response = await app.login();
  assert.equal(response.status, 200);
  assert.match(response.headers.get('set-cookie'), /^orbit_session=.+; Path=\/; HttpOnly; SameSite=Strict$/);
  assert.equal(response.headers.get('cache-control'), 'no-store');
  const token = app.cookie.split('=')[1];
  const session = app.database.prepare('SELECT * FROM sessions').get();
  assert.equal(session.token_hash, createHash('sha256').update(token).digest('hex'));
  assert.equal((await (await app.request('/api/auth/status')).json()).authenticated, true);
  const logout = await app.request('/api/auth/logout', { method: 'POST' });
  assert.equal(logout.status, 204);
  assert.match(logout.headers.get('set-cookie'), /Max-Age=0/);
  assert.equal((await app.request('/api/orgs')).status, 401);
});

test('org creation, switching, selected org and login survive an actual server/database restart', async t => {
  const app = await fixture(t);
  await app.configure();
  await app.login();
  assert.deepEqual(await (await app.request('/api/orgs')).json(), { orgs: [], active_org_id: null });
  const firstResponse = await app.request('/api/orgs', { json: { name: '  我的工作室  ' } });
  assert.equal(firstResponse.status, 201);
  const first = await firstResponse.json();
  assert.equal(first.orgs[0].name, '我的工作室');
  assert.equal(first.active_org_id, first.orgs[0].id);
  const second = await (await app.request('/api/orgs', { json: { name: "York's Lab <script>" } })).json();
  assert.equal(second.orgs.length, 2);
  const switched = await app.request('/api/orgs/active', { method: 'PUT', json: { id: first.active_org_id } });
  assert.equal(switched.status, 200);
  await app.restart();
  const restored = await (await app.request('/api/orgs')).json();
  assert.equal(restored.orgs.length, 2);
  assert.equal(restored.active_org_id, first.active_org_id);
  assert.equal(restored.orgs.find(org => org.id === second.active_org_id).name, "York's Lab <script>");
  assert.equal((await app.request('/api/orgs/active', { method: 'PUT', json: { id: 'missing' } })).status, 404);
  assert.equal((await (await app.request('/api/orgs')).json()).active_org_id, first.active_org_id);
  const files = await readFile(join(app.directory, 'orbit.sqlite'));
  assert.equal(files.subarray(0, 15).toString(), 'SQLite format 3');
});

test('invalid orgs, foreign origins, malformed JSON and oversized bodies cannot create data', async t => {
  const app = await fixture(t);
  await app.configure();
  assert.equal((await app.request('/api/auth/login', { headers: { Origin: 'https://evil.example' }, json: {} })).status, 403);
  await app.login();
  for (const name of ['', '   ', 'a'.repeat(81), 'bad\nname', null, 123]) {
    assert.equal((await app.request('/api/orgs', { json: { name } })).status, 422);
  }
  assert.equal((await app.request('/api/orgs', { json: { name: 'forbidden' }, headers: { Origin: 'null' } })).status, 403);
  assert.equal((await app.request('/api/orgs', { json: { name: 'forbidden' }, headers: { 'Sec-Fetch-Site': 'cross-site' } })).status, 403);
  assert.equal((await app.request('/api/orgs', { method: 'POST', body: '{', headers: { 'Content-Type': 'application/json' } })).status, 400);
  assert.equal((await app.request('/api/orgs', { method: 'POST', body: 'plain' })).status, 415);
  assert.equal((await app.request('/api/orgs', { json: { name: 'x'.repeat(17_000) } })).status, 413);
  assert.deepEqual(await (await app.request('/api/orgs')).json(), { orgs: [], active_org_id: null });
});

test('passive status does not refresh idle expiry; account changes revoke sessions but keep orgs', async t => {
  const app = await fixture(t);
  await app.configure();
  await app.login();
  const created = await (await app.request('/api/orgs', { json: { name: 'Persistent' } })).json();
  const before = Date.now() - 5000;
  app.database.prepare('UPDATE sessions SET last_active = ?').run(before);
  await app.request('/api/auth/status');
  assert.equal(app.database.prepare('SELECT last_active FROM sessions').get().last_active, before);
  await app.request('/api/orgs');
  assert.ok(app.database.prepare('SELECT last_active FROM sessions').get().last_active > before);
  for (const column of ['last_active', 'created_at']) {
    app.database.exec(`UPDATE sessions SET ${column} = 0`);
    assert.equal((await app.request('/api/orgs')).status, 401);
    await app.login();
  }
  await setAccount(app.database, 'updated-user', 'updated-password');
  assert.equal((await app.request('/api/orgs')).status, 401);
  assert.equal((await app.login('updated-user', 'updated-password')).status, 200);
  assert.equal((await (await app.request('/api/orgs')).json()).active_org_id, created.active_org_id);
});

test('login attempts are bounded and explicit frontend origins support separate deployment', async t => {
  const app = await fixture(t, { frontendOrigin: 'http://127.0.0.1:5174', secureCookies: true });
  await app.configure();
  const preflight = await app.request('/api/orgs', { method: 'OPTIONS', headers: { Origin: 'http://127.0.0.1:5174' } });
  assert.equal(preflight.status, 204);
  assert.equal(preflight.headers.get('access-control-allow-origin'), 'http://127.0.0.1:5174');
  assert.equal(preflight.headers.get('access-control-allow-credentials'), 'true');
  const responses = await Promise.all(Array.from({ length: 3 }, () => app.login()));
  assert.equal(responses.filter(response => response.status === 200).length, 2);
  const limited = responses.find(response => response.status === 429);
  assert.ok(limited);
  assert.ok(Number(limited.headers.get('retry-after')) >= 1);
  assert.match(responses.find(response => response.status === 200).headers.get('set-cookie'), /; Secure$/);
});

test('serves built frontend with security headers and keeps unknown API and asset requests out of SPA fallback', async t => {
  const app = await fixture(t);
  const page = await app.request('/');
  assert.equal(page.status, 200);
  assert.match(page.headers.get('content-type'), /text\/html/);
  assert.match(page.headers.get('content-security-policy'), /frame-ancestors 'none'/);
  assert.match(await page.text(), /Orbit/);
  assert.equal((await app.request('/assets/missing.js')).status, 404);
  assert.equal((await app.request('/api/unknown')).status, 401);
  assert.equal((await app.request('/%2e%2e%2fsecret.txt')).status, 404);
  assert.equal((await app.request('/%00')).status, 400);
});
