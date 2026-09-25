// Uses the real Rust server and the same production frontend build for both
// mounts. Build first: cargo build -p aow-server && cd frontend && npm run build
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { chromium } from 'playwright';
import { snapshot } from './fixtures/session-snapshot.mjs';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
  ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

async function startServer(t, base, { envBase = base, args = [] } = {}) {
  const state = await mkdtemp(join(tmpdir(), 'aow-base-path-'));
  let child;
  t.after(async () => {
    if (child && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, 'exit');
      child.kill('SIGTERM');
      const timer = setTimeout(() => child.kill('SIGKILL'), 3000);
      try { await exited; } finally { clearTimeout(timer); }
    }
    await rm(state, { recursive: true, force: true });
  });
  await writeFile(join(state, 'pin.md5'), createHash('md5').update('123456').digest('hex'));
  // Keep all initialization writes inside this test's temporary directory.
  await writeFile(join(state, 'aow-settings.json'), JSON.stringify({ version: 1, notes_base: join(state, 'notes') }));
  child = spawn(process.env.AOW_TEST_SERVER ?? join(repo, 'target/debug/aow-server'), [
    '--host', '127.0.0.1', '--port', '0', '--state-dir', state,
    '--terminald-socket', join(state, 'unused.sock'), '--frontend', join(repo, 'frontend/dist'),
    ...args,
  ], { cwd: repo, env: { ...process.env, AOW_BASE_PATH: envBase }, stdio: ['ignore', 'pipe', 'pipe'] });
  const origin = await new Promise((resolve, reject) => {
    let output = '';
    const timer = setTimeout(() => reject(new Error(`Server startup timed out: ${output}`)), 15000);
    const finish = error => { clearTimeout(timer); reject(error); };
    child.once('error', finish);
    child.once('exit', code => finish(new Error(`Server exited ${code}: ${output}`)));
    const read = chunk => {
      output += chunk.toString();
      const match = /AoW: (http:\/\/127\.0\.0\.1:\d+)/.exec(output);
      if (match) { clearTimeout(timer); resolve(match[1]); }
    };
    child.stdout.on('data', read);
    child.stderr.on('data', read);
  });
  const response = await fetch(`${origin}${base}/api/health`, { signal: AbortSignal.timeout(15000) });
  assert.equal(response.status, 200);
  await response.text();
  return origin;
}

try {
  for (const base of ['', '/tools/aow']) {
    await test(`production build works at ${base || '/'}: login, deep links, assets, streams and mobile share`, { timeout: 60000 }, async t => {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      t.after(() => context.close());
      const origin = await startServer(t, base);
      const requests = new Set();
      const sockets = [];
      const errors = [];
      const root = '/workspace/fixture';
      const project = { id: 'project', name: 'Prefix Fixture', registered_path: root, common_git_dir: `${root}/.git`, notes_path: '/notes',
        worktrees: [{ id: 'wt', project_id: 'project', path: root, branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' }] };
      const terminal = { id: 'target', name: 'Prefix Terminal', name_is_custom: true, workspace_root: root, revision: 1,
        layout: { type: 'pane', pane_id: 'pane' }, panes: [{ id: 'pane', name: 'Shell', cwd: root, kind: 'terminal', shell: '/bin/bash', status: 'running', rows: 24, cols: 80 }] };
      await context.route('**/api/**', async route => {
        const pathname = new URL(route.request().url()).pathname;
        if (!pathname.startsWith(`${base}/api/`)) return route.continue();
        const path = pathname.slice(base.length);
        // Authentication, cookies and streams go through the real backend.
        if (path.startsWith('/api/auth/') || ['/api/operations', '/api/operations/stream', '/api/terminals/task-stops'].includes(path)) return route.continue();
        let data = [];
        if (path === '/api/aow/projects') data = [project];
        else if (path === '/api/aow/settings') data = { notes_base: '/notes', execution_path: ['/usr/bin'], node_addresses: [] };
        else if (path.includes('/pinned-')) data = { paths: [], revision: 0 };
        else if (path === '/api/terminals') data = [terminal];
        else if (path === '/api/terminals/target') data = terminal;
        else if (path === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
        else if (path === '/api/git/status') data = { repository: root, branch: 'main', files: [] };
        else if (path === '/api/git/ignored') data = { repository: root, ignored: [] };
        else if (path === '/api/git/log') data = { repository: root, commits: [], upstream: null };
        else if (path.startsWith('/api/fs/tree')) data = { path: root, entries: [] };
        else if (path === '/api/public/session-shares/token') data = snapshot;
        await route.fulfill({ json: data });
      });
      await context.routeWebSocket('**/api/terminals/**', socket => {
        sockets.push(new URL(socket.url()).pathname);
        socket.onMessage(message => {
          if (typeof message !== 'string' || JSON.parse(message).type !== 'claim') return;
          socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
          socket.send(JSON.stringify({ type: 'stream', epoch: 'fixture', offset: 0, reset: true, replay_bytes: 0,
            restore_cols: 80, restore_rows: 24, restore: '\x1b[2J\x1b[HPrefix terminal\r\n$ ' }));
        });
      });
      const page = await context.newPage();
      page.setDefaultTimeout(15000);
      page.on('pageerror', error => errors.push(error.message));
      page.on('request', request => requests.add(new URL(request.url()).pathname));
      await page.goto(`${origin}${base}/aow/tabs/terminal/target?ui=desktop`);
      await page.getByLabel('PIN 码').fill('123456');
      const activeTab = page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab.active').filter({ hasText: 'Prefix Terminal' });
      await activeTab.waitFor();
      await page.locator('.terminal-emulator-shell:not(.restore-pending)').first().waitFor();
      await page.reload();
      await activeTab.waitFor();
      assert.equal(await page.locator('base').getAttribute('href'), `${base}/`);
      assert.equal(await page.locator('meta[name="aow-base-path"]').getAttribute('content'), base);
      assert.equal(await page.locator('[aria-label="产品介绍（新窗口打开）"]').getAttribute('href'), 'https://yorkart.github.io/aow/');
      assert.equal((await context.cookies()).find(cookie => cookie.name.startsWith('aow_session'))?.path, `${base}/`);
      assert.ok(sockets.length > 0);
      assert.ok(sockets.every(path => path === `${base}/api/terminals/target/panes/pane/ws`));
      assert.ok(requests.has(`${base}/api/terminals/task-stops`));
      assert.ok(requests.has(`${base}/api/operations/stream`));
      assert.ok([...requests].some(path => path.startsWith(`${base}/assets/`) && path.endsWith('.js')));
      assert.ok(await page.evaluate(() => Object.keys(localStorage).some(key => key.endsWith('aow-active')
        && key.startsWith(document.querySelector('meta[name="aow-base-path"]').content ? 'aow@' : 'aow-active'))));
      await page.goto(`${origin}${base}/m/`);
      await page.getByRole('link', { name: '打开桌面版' }).waitFor();
      assert.equal(await page.getByRole('link', { name: '打开桌面版' }).getAttribute('href'), `${base}/?ui=desktop`);
      await context.clearCookies();
      await page.goto(`${origin}${base}/share/token`);
      await page.getByText('会话分享 · 只读').waitFor();
      assert.equal(await page.getByLabel('PIN 码').count(), 0);
      assert.ok(requests.has(`${base}/api/public/session-shares/token`));
      assert.deepEqual(errors, []);
      if (base) assert.deepEqual([...requests].filter(path => /^\/(api|assets|aow|m|share)(\/|$)/.test(path)), []);
    });
  }
  await test('CLI base path overrides the environment and normalizes its trailing slash', async t => {
    const origin = await startServer(t, '/chosen', { envBase: '/environment', args: ['--base-path', '/chosen/'] });
    const wrong = await fetch(`${origin}/environment/api/health`);
    assert.equal(wrong.status, 404);
    await wrong.text();
    const redirect = await fetch(`${origin}/chosen?ui=mobile`, { redirect: 'manual' });
    assert.equal(redirect.status, 308);
    assert.equal(redirect.headers.get('location'), '/chosen/?ui=mobile');
    await redirect.text();
  });
} finally {
  await browser.close();
}
