import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { mkdir } from 'node:fs/promises';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { makeTurn, session, snapshot } from './fixtures/session-snapshot.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await mkdir('/tmp/aow-session-share', { recursive: true });

  async function pageFor(t, mobile = false) {
    const context = await browser.newContext({ viewport: { width: mobile ? 390 : 1280, height: mobile ? 844 : 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
    t.after(() => context.close());
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(() => assert.deepEqual(errors, []));
    return page;
  }

  for (const mobile of [false, true]) {
    await test(`${mobile ? 'mobile' : 'desktop'} owners create, copy, restore and revoke a share`, async t => {
      const page = await pageFor(t, mobile);
      let stored = null;
      let creations = 0;
      await page.route('**/api/**', async route => {
        const request = route.request();
        const path = new URL(request.url()).pathname;
        if (path.endsWith('/snapshot')) return route.fulfill({ json: snapshot });
        if (path.endsWith('/share') && request.method() === 'GET') return route.fulfill({ json: stored });
        if (path.endsWith('/share') && request.method() === 'POST') {
          assert.deepEqual(request.postDataJSON(), { agent: session.agent, worktree_path: session.cwd });
          creations += 1;
          stored ??= { id: `share-${creations}`, path: `/share/token-${creations}`, created_at: new Date().toISOString() };
          return route.fulfill({ json: stored });
        }
        if (path.startsWith('/api/aow/session-shares/') && request.method() === 'DELETE') {
          stored = null;
          return route.fulfill({ status: 204 });
        }
        return route.fulfill({ status: 404, json: { message: 'unexpected request' } });
      });
      await page.goto(`${origin}/tests/session-snapshot-preview.html${mobile ? '?mobile' : ''}`);
      await page.getByRole('button', { name: '分享会话', exact: true }).click();
      await page.getByText('持有链接的人无需登录', { exact: false }).waitFor();
      await page.getByRole('button', { name: '创建并复制链接' }).click();
      await page.getByText('链接已复制', { exact: true }).waitFor();
      assert.equal(await page.getByLabel('分享链接', { exact: true }).inputValue(), `${origin}/share/token-1`);
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${origin}/share/token-1`);
      assert.equal(await page.locator('.session-share-dialog').evaluate(node => node.getBoundingClientRect().width <= innerWidth), true);
      await page.screenshot({ path: `/tmp/aow-session-share/dialog-${mobile ? 'mobile' : 'desktop'}.png` });
      await page.getByRole('button', { name: '关闭分享窗口' }).click();
      await page.getByRole('button', { name: '分享会话', exact: true }).click();
      await page.getByRole('button', { name: '复制链接', exact: true }).click();
      await page.getByText('链接已复制', { exact: true }).waitFor();
      assert.equal(creations, 1);
      await page.getByRole('button', { name: '取消分享', exact: true }).click();
      await page.getByText('已取消分享，旧链接已失效').waitFor();
      await page.getByRole('button', { name: '创建并复制链接' }).click();
      await page.getByText('链接已复制', { exact: true }).waitFor();
      assert.equal(await page.getByLabel('分享链接', { exact: true }).inputValue(), `${origin}/share/token-2`);
    });
  }

  for (const mobile of [false, true]) {
    await test(`${mobile ? 'mobile' : 'desktop'} public entry skips login, polls, pauses when hidden and clears revoked content`, async t => {
      const page = await pageFor(t, mobile);
      await page.clock.install();
      let current = structuredClone(snapshot);
      let revoked = false;
      let reads = 0;
      const unexpected = [];
      await page.route('**/api/**', async route => {
        const path = new URL(route.request().url()).pathname;
        if (path === '/api/public/session-shares/public-token') {
          reads += 1;
          assert.equal(route.request().headers().cookie, undefined);
          return revoked ? route.fulfill({ status: 404, json: { message: '分享链接无效或已取消' } }) : route.fulfill({ json: current });
        }
        unexpected.push(path);
        return route.fulfill({ status: 401, json: { message: 'requires login' } });
      });
      await page.goto(`${origin}/share/public-token`);
      await page.getByText('会话分享 · 只读').waitFor();
      await page.locator('.project-aow-snapshot-conclusion').first().waitFor();
      assert.equal(await page.getByRole('button', { name: '分享会话', exact: true }).count(), 0);
      assert.equal(await page.locator('.aow-auth').count(), 0);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
      assert.equal(await page.evaluate(() => window.__unsafe), undefined);
      await page.screenshot({ path: `/tmp/aow-session-share/public-${mobile ? 'mobile' : 'desktop'}.png` });
      current.turns.push(makeTurn('shared-new', '分享后的新问题', '分享后的新回复', []));
      await page.clock.runFor(10_100);
      await page.getByText('分享后的新回复', { exact: true }).waitFor();
      await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' }); document.dispatchEvent(new Event('visibilitychange')); });
      const beforeHidden = reads;
      await page.clock.runFor(30_000);
      assert.equal(reads, beforeHidden);
      revoked = true;
      await page.evaluate(() => { Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' }); document.dispatchEvent(new Event('visibilitychange')); });
      await page.getByText('分享链接无效或已取消').waitFor();
      assert.equal(await page.locator('.project-aow-snapshot-conclusion').count(), 0);
      const afterRevoked = reads;
      await page.clock.runFor(30_000);
      assert.equal(reads, afterRevoked);
      assert.deepEqual(unexpected, []);
    });
  }

  await test('public reading retries transient errors and an ordinary entry still requires PIN', async t => {
    const page = await pageFor(t);
    let failed = true;
    await page.route('**/api/**', async route => {
      const path = new URL(route.request().url()).pathname;
      if (path.startsWith('/api/public/session-shares/')) return failed
        ? route.fulfill({ status: 503, json: { message: '读取暂不可用' } }) : route.fulfill({ json: snapshot });
      if (path === '/api/auth/status') return route.fulfill({ json: { configured: true, authenticated: false } });
      return route.fulfill({ status: 401, json: { message: 'requires login' } });
    });
    await page.goto(`${origin}/share/retry-token`);
    await page.getByText('读取暂不可用').waitFor();
    failed = false;
    await page.getByRole('button', { name: '重试', exact: true }).click();
    await page.locator('.project-aow-snapshot-conclusion').first().waitFor();
    await page.goto(`${origin}/aow/?token=retry-token`);
    await page.getByRole('heading', { name: '输入访问 PIN' }).waitFor();
  });

  await test('a fast visibility change replaces an in-flight read immediately', async t => {
    const page = await pageFor(t);
    await page.clock.install();
    let reads = 0;
    let holdNext = false;
    let currentTitle = snapshot.title;
    let release;
    const pending = new Promise(resolve => { release = resolve; });
    let started;
    const pendingStarted = new Promise(resolve => { started = resolve; });
    t.after(() => release());
    await page.route('**/api/public/session-shares/**', async route => {
      reads += 1;
      if (holdNext) { holdNext = false; started(); await pending; }
      return route.fulfill({ json: { ...snapshot, title: currentTitle } });
    });
    await page.goto(`${origin}/share/inflight-token`);
    await page.locator('.project-aow-snapshot-conclusion').first().waitFor();
    const before = reads;
    holdNext = true;
    await page.getByRole('button', { name: '刷新会话快照' }).click();
    await pendingStarted;
    currentTitle = '恢复可见后的新内容';
    await page.evaluate(() => {
      for (const value of ['hidden', 'visible']) {
        Object.defineProperty(document, 'visibilityState', { configurable: true, value });
        document.dispatchEvent(new Event('visibilitychange'));
      }
    });
    await page.getByRole('heading', { name: '恢复可见后的新内容' }).waitFor({ timeout: 3000 });
    release();
    assert.equal(reads, before + 2);
  });
} finally {
  await browser?.close();
  await server.close();
}
