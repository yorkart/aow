// Real server/browser regression coverage for the authentication security boundary.
// Build first with just build. All files, sockets and shell commands stay in a fixture directory.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { access, mkdtemp, mkdir, writeFile, rename, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';
import { chromium } from 'playwright';
import WebSocket from 'ws';
import { credentials } from '../../scripts/tests/account-fixture.mjs';

const repo = fileURLToPath(new URL('../../', import.meta.url));
async function exists(path) { try { await access(path); return true; } catch { return false; } }
async function until(check, label) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) { if (await check()) return; await delay(30); }
  throw new Error(`Timed out: ${label}`);
}
async function bounded(promise, label, milliseconds = 5000) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(label)), milliseconds); })]); }
  finally { clearTimeout(timer); }
}
async function fixture(t, { secure = false, base = '' } = {}) {
  const root = await mkdtemp(join(tmpdir(), 'aow-auth-security-'));
  const state = join(root, 'state');
  const children = [];
  t.after(async () => {
    for (const child of children.reverse()) {
      if (child.exitCode === null && child.signalCode === null) {
        const exited = once(child, 'exit'); child.kill('SIGTERM');
        const timer = setTimeout(() => child.kill('SIGKILL'), 3000);
        try { await exited; } finally { clearTimeout(timer); }
      }
    }
    await rm(root, { recursive: true, force: true });
  });
  await mkdir(state);
  const setPassword = async password => {
    const pending = join(state, 'credentials.pending');
    await writeFile(pending, credentials(password), { mode: 0o600 });
    await rename(pending, join(state, 'credentials.json'));
  };
  await setPassword('test-password');
  await writeFile(join(state, 'aow-settings.json'), JSON.stringify({ version: 1, notes_base: join(root, 'notes') }));
  const start = (name, args) => {
    const child = spawn(join(repo, 'target/debug', name), args, { cwd: repo,
      env: { ...process.env, AOW_BASE_PATH: base, AOW_AUTH_SECURE_COOKIE: String(secure) }, stdio: ['ignore', 'pipe', 'pipe'] });
    child.output = '';
    child.stdout.on('data', chunk => child.output += chunk);
    child.stderr.on('data', chunk => child.output += chunk);
    children.push(child); return child;
  };
  const socket = join(root, 'term.sock');
  start('aow-terminald', ['--socket', socket, '--vt-node', process.execPath]);
  await until(() => exists(socket), 'terminald socket');
  const server = start('aow-server', ['--host', '127.0.0.1', '--port', '0', '--state-dir', state,
    '--terminald-socket', socket, '--frontend', join(repo, 'frontend/dist')]);
  let origin;
  await until(() => { origin = /AoW: (http:\/\/127\.0\.0\.1:\d+)/.exec(server.output)?.[1]; return !!origin; }, 'server startup');
  const url = path => origin + base + path;
  assert.equal((await fetch(url('/api/health'))).status, 200, server.output);
  const login = async (password = 'test-password', extraHeaders = {}) => fetch(url('/api/auth/login'), { method: 'POST',
    headers: { 'content-type': 'application/json', ...extraHeaders }, body: JSON.stringify({ method: 'password', username: 'admin', password }) });
  return { root, url, origin, base, setPassword, login };
}

for (const action of ['password', 'password-with-immediate-input', 'logout']) {
  await test(`${action} revokes established terminal and event streams, not only new HTTP requests`, { timeout: 30000 }, async t => {
    const f = await fixture(t);
    const logged = await f.login();
    assert.equal(logged.status, 200);
    const cookie = logged.headers.get('set-cookie').split(';')[0];
    const created = await fetch(f.url('/api/terminals'), { method: 'POST', headers: { cookie, 'content-type': 'application/json' },
      body: JSON.stringify({ cwd: f.root, workspace_root: f.root, shell: '/bin/sh', rows: 24, cols: 80 }) });
    assert.equal(created.status, 201, await created.clone().text());
    const tab = await created.json();
    const ws = new WebSocket(f.url(`/api/terminals/${tab.id}/panes/${tab.panes[0].id}/ws`).replace('http:', 'ws:'), { headers: { cookie, origin: f.origin } });
    t.after(() => ws.terminate());
    let messages = 0;
    ws.on('message', () => messages++);
    await once(ws, 'open');
    await until(() => messages > 0, 'terminal ready');
    const events = await fetch(f.url('/api/operations/stream'), { headers: { cookie } });
    const reader = events.body.getReader();
    t.after(() => reader.cancel());
    assert.equal((await bounded(reader.read(), 'initial event')).done, false);
    const closed = once(ws, 'close');
    const live = new WebSocket(f.url('/api/events/ws').replace('http:', 'ws:'), { headers: { cookie, origin: f.origin } });
    t.after(() => live.terminate());
    const liveReady = once(live, 'message');
    await once(live, 'open');
    assert.equal(JSON.parse((await liveReady)[0].toString()).event, 'workspace');
    const liveClosed = once(live, 'close');
    if (action === 'logout') {
      const logout = await fetch(f.url('/api/auth/logout'), { method: 'POST', headers: { cookie } });
      assert.equal(logout.status, 204);
      assert.match(logout.headers.get('set-cookie'), /Max-Age=0/);
    } else {
      await f.setPassword('replacement-password');
    }
    const marker = join(f.root, 'must-not-be-created.txt');
    if (action === 'password-with-immediate-input' && ws.readyState === WebSocket.OPEN) {
      ws.send(Buffer.from(`printf revoked > '${marker}'\n`));
    }
    await bounded(closed, 'revoked WebSocket was not closed');
    assert.equal((await bounded(liveClosed, 'revoked event WebSocket was not closed'))[0], 1008);
    await bounded((async () => { while (!(await reader.read()).done) {} })(), 'revoked SSE did not end');
    assert.equal(await exists(marker), false);
    assert.equal((await fetch(f.url('/api/fs/tree'), { headers: { cookie } })).status, 401);
    const fresh = await f.login(action === 'logout' ? 'test-password' : 'replacement-password');
    assert.equal(fresh.status, 200);
    const current = fresh.headers.get('set-cookie').split(';')[0];
    assert.equal((await fetch(f.url(`/api/terminals/${tab.id}`), { method: 'DELETE', headers: { cookie: current } })).status, 204);
  });
}

await test('failed logins back off and parallel password verification has bounded admission', { timeout: 30000 }, async t => {
  const f = await fixture(t);
  for (let index = 0; index < 5; index++) assert.equal((await f.login('wrong-password')).status, 401);
  const limited = await f.login('wrong-password', { 'x-forwarded-for': '192.0.2.123' });
  assert.equal(limited.status, 429);
  assert.equal(limited.headers.has('set-cookie'), false);
  const retry = Number(limited.headers.get('retry-after'));
  assert.ok(retry >= 1 && retry <= 60);
  await delay(retry * 1000 + 30);
  const concurrent = await Promise.all(Array.from({ length: 3 }, () => f.login()));
  assert.deepEqual(concurrent.map(response => response.status).sort(), [200, 200, 429]);
});

await test('HTTPS cookie configuration survives a subpath login and logout', { timeout: 15000 }, async t => {
  const f = await fixture(t, { secure: true, base: '/tools/aow' });
  const login = await f.login('test-password', { 'x-forwarded-proto': 'http' });
  assert.equal(login.status, 200);
  assert.match(login.headers.get('set-cookie'), /Path=\/tools\/aow\/; HttpOnly; SameSite=Strict; Secure/);
  assert.equal(login.headers.get('cache-control'), 'no-store');
  const cookie = login.headers.get('set-cookie').split(';')[0];
  const logout = await fetch(f.url('/api/auth/logout'), { method: 'POST', headers: { cookie } });
  assert.equal(logout.status, 204);
  assert.match(logout.headers.get('set-cookie'), /Path=\/tools\/aow\/; HttpOnly; SameSite=Strict; Secure; Max-Age=0/);
  assert.equal((await fetch(f.url('/api/fs/tree'), { headers: { cookie } })).status, 401);
});

const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
try {
  await test('untrusted HTML and SVG download without authenticated script execution; images still render', { timeout: 30000 }, async t => {
    const f = await fixture(t);
    const context = await browser.newContext(); t.after(() => context.close());
    assert.equal((await context.request.post(f.url('/api/auth/login'), { data: { username: 'admin', password: 'test-password' } })).status(), 200);
    const page = await context.newPage();
    await page.goto(f.url('/aow/?ui=mobile'));
    await page.getByRole('button', { name: '退出登录' }).waitFor();
    const marker = join(f.root, 'must-not-be-written.txt');
    const script = `fetch(${JSON.stringify('/api/fs/file' + marker)}, {method:'PUT',body:'unsafe'})`;
    for (const [name, content] of [
      ['untrusted.html', `<script>${script}</script>`],
      ['untrusted.svg', `<svg xmlns="http://www.w3.org/2000/svg"><script><![CDATA[${script}]]></script></svg>`],
    ]) {
      const path = join(f.root, name); await writeFile(path, content);
      const url = f.url('/api/fs/raw' + path);
      const response = await context.request.get(url);
      assert.match(response.headers()['content-disposition'], /^attachment;/);
      assert.equal(response.headers()['content-security-policy'], "sandbox; default-src 'none'");
      const downloaded = page.waitForEvent('download');
      await page.evaluate(url => { const link = document.createElement('a'); link.href = url; document.body.append(link); link.click(); link.remove(); }, url);
      assert.equal((await downloaded).suggestedFilename(), name);
    }
    assert.equal(await exists(marker), false);
    const png = join(f.root, 'pixel.png');
    await writeFile(png, Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a3FoAAAAASUVORK5CYII=', 'base64'));
    assert.equal(await page.evaluate(url => new Promise(resolve => {
      const image = new Image(); image.onload = () => resolve(image.naturalWidth); image.onerror = () => resolve(0); image.src = url;
    }), f.url('/api/fs/raw' + png)), 1);
  });
  await test('PDF keeps its inline media response without plugin-blocking sandbox headers', { timeout: 30000 }, async t => {
    const f = await fixture(t);
    const context = await browser.newContext(); t.after(() => context.close());
    await context.request.post(f.url('/api/auth/login'), { data: { username: 'admin', password: 'test-password' } });
    const page = await context.newPage();
    await page.setContent('<p>PDF preview fixture</p>');
    const path = join(f.root, 'preview.pdf');
    await writeFile(path, await page.pdf());
    const response = await context.request.get(f.url('/api/fs/raw' + path));
    assert.equal(response.status(), 200);
    assert.equal(response.headers()['content-type'], 'application/pdf');
    assert.equal(response.headers()['content-disposition'], undefined);
    assert.equal(response.headers()['content-security-policy'], undefined);
    assert.equal(response.headers()['x-content-type-options'], 'nosniff');
    // Headless Shell has no PDF viewer. Check the response contract here;
    // do not mistake its automatic PDF download for an application regression.
    assert.match((await response.body()).subarray(0, 5).toString(), /^%PDF-/);
  });
  for (const mobile of [false, true]) {
    await test(`${mobile ? 'mobile' : 'desktop'} logout UI clears the session and returns to login`, { timeout: 30000 }, async t => {
      const f = await fixture(t, { base: '/tools/aow' });
      const context = await browser.newContext({ viewport: { width: mobile ? 390 : 1440, height: 900 } });
      t.after(() => context.close());
      assert.equal((await context.request.post(f.url('/api/auth/login'), { data: { username: 'admin', password: 'test-password' } })).status(), 200);
      const page = await context.newPage();
      await page.goto(f.url(`/aow/?ui=${mobile ? 'mobile' : 'desktop'}`));
      if (!mobile) await page.getByRole('button', { name: 'Settings', exact: true }).click();
      await page.getByRole('button', { name: '退出登录', exact: true }).click();
      await page.getByRole('heading', { name: '登录 AoW', exact: true }).waitFor();
      assert.equal((await context.request.get(f.url('/api/fs/tree'))).status(), 401);
      assert.equal((await context.cookies()).some(cookie => cookie.name.startsWith('aow_session')), false);
    });
  }
} finally { await browser.close(); }
