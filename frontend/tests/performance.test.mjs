import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
const files = Array.from({ length: 20000 }, (_, i) => ({ path: `dir-${String(Math.floor(i / 1000)).padStart(2, '0')}/file-${String(i).padStart(5, '0')}.ts`, index_status: i === 0 ? 'M' : ' ', worktree_status: 'M', original_path: null }));
const commits = [0, 1].map(i => ({ id: `commit-${i}`, short_id: `abc${i}`, subject: `Commit ${i}`, author: 'Fixture', authored_at: '2026-09-16T00:00:00Z', parents: i === 0 ? ['commit-1'] : [], is_pushed: true }));
let browser;
try {
  await server.listen();
  const baseURL = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function fixture(t) {
    const context = await browser.newContext({ viewport: { width: 1100, height: 900 }, reducedMotion: 'reduce' });
    const state = { errors: [], paths: ['/a', '/b'], pinRequests: 0, pinGate: null, failPins: false, gitRequests: [], statusGate: null, logGate: null, failStatus: false, branch: 'main', commits: structuredClone(commits) };
    state.gitCommands = [];
    state.commandGate = null;
    state.commandError = '';
    state.abortCommand = false;
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); });
    await context.route('**/api/**', async route => {
      const url = new URL(route.request().url());
      let data;
      if (url.pathname === '/api/aow/pinned-worktrees') {
        state.pinRequests += 1;
        await state.pinGate;
        if (state.failPins) { await route.fulfill({ status: 503, json: { message: 'Fixture unavailable' } }); return; }
        if (route.request().method() === 'PATCH') {
          const { add = [], remove = [], order } = route.request().postDataJSON();
          state.paths = order ?? [...new Set([...state.paths.filter(path => !remove.includes(path)), ...add])];
        }
        data = { paths: state.paths };
      } else if (url.pathname === '/api/git/status') {
        state.gitRequests.push('status');
        data = { repository: '/fixture', branch: state.branch, ahead: 0, behind: 0, files };
        await state.statusGate;
        if (state.failStatus) { await route.fulfill({ status: 503, json: { message: 'Status unavailable' } }); return; }
      } else if (url.pathname === '/api/git/log') {
        state.gitRequests.push('log');
        data = { repository: '/fixture', upstream: null, upstream_commit: null, commits: state.commits };
        await state.logGate;
      }
      else if (url.pathname === '/api/git/pull' || url.pathname === '/api/git/push') {
        state.gitCommands.push({ command: url.pathname.split('/').at(-1), method: route.request().method(), ...route.request().postDataJSON() });
        await state.commandGate;
        if (state.abortCommand) { await route.abort('failed'); return; }
        await route.fulfill(state.commandError ? { status: 400, json: { message: state.commandError } } : { status: 204 });
        return;
      }
      else if (url.pathname === '/api/git/commit/files') {
        await state.commitFilesGate;
        data = { repository: '/fixture', commit: url.searchParams.get('commit'), files: state.commitFiles ?? files.map(file => ({ path: file.path, status: 'M', original_path: null })) };
      }
      await route.fulfill(data === undefined ? { status: 404, json: { message: 'Unknown fixture endpoint' } } : { json: data });
    });
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    page.on('pageerror', error => state.errors.push(error.message));
    await page.clock.install({ time: new Date('2026-09-16T00:00:00Z') });
    await page.goto(`${baseURL}/tests/performance-preview.html`);
    await page.locator('.changes-list .change-row').first().waitFor();
    await page.waitForFunction(() => !document.querySelector('button').disabled);
    return { page, state };
  }

  async function navigate(page, title) {
    await page.getByRole('button', { name: `定位 ${title}`, exact: true }).click();
    await page.clock.runFor(32);
  }

  await test('commit graph keeps HEAD as the mainline and connects across expanded file lists', async t => {
    const { page, state } = await fixture(t);
    state.commits = [
      { id: 'head', parents: ['main', 'feature'], subject: 'Merge feature' },
      { id: 'feature', parents: ['base'], subject: 'Feature change' },
      { id: 'main', parents: ['base'], subject: 'Main change' },
      { id: 'base', parents: [], subject: 'Common base' },
    ].map(value => ({ ...commits[0], ...value, short_id: value.id }));
    state.commitFiles = [{ path: 'frontend/src/styles.css', status: 'M', original_path: null }];
    await page.getByRole('button', { name: '收起 Changes', exact: true }).click();
    await page.getByRole('button', { name: '刷新 Commits', exact: true }).click();
    await page.locator('.commit-row code').filter({ hasText: 'head' }).waitFor();
    await navigate(page, 'Commits');
    const geometry = await page.locator('.commit-node').evaluateAll(nodes => nodes.map(node => {
      const dot = node.querySelector('circle');
      const graph = node.querySelector('.commit-graph-layer').getBoundingClientRect();
      return { x: dot.cx.baseVal.value, color: getComputedStyle(dot).stroke,
        clear: node.querySelector('.commit-disclosure').getBoundingClientRect().left >= graph.right };
    }));
    for (const index of [2, 3]) {
      assert.equal(geometry[index].x, geometry[0].x);
      assert.equal(geometry[index].color, geometry[0].color);
    }
    assert.ok(geometry[1].x > geometry[0].x);
    assert.ok(geometry.every(row => row.clear), 'graph lanes must not overlap labels or disclosure controls');

    async function connected() {
      const measurements = await page.locator('.commit-node').evaluateAll(nodes => nodes.slice(0, -1).flatMap((node, index) => {
        const svg = node.querySelector('svg').getBoundingClientRect();
        const next = nodes[index + 1].getBoundingClientRect();
        const lines = [...node.querySelectorAll('.commit-graph-continuation')];
        return lines.length ? lines.map(line => {
          const box = line.getBoundingClientRect();
          return { startGap: box.top - svg.bottom, endGap: next.top - box.bottom, width: box.width };
        }) : [{ startGap: 0, endGap: next.top - svg.bottom, width: 1 }];
      }));
      assert.ok(measurements.every(line => Math.abs(line.startGap) < .1 && Math.abs(line.endGap) < .1 && line.width === 1), JSON.stringify(measurements));
      assert.ok(await page.locator('.commit-graph-edge').evaluateAll(edges => edges.every(edge => getComputedStyle(edge).strokeWidth === '1px')));
    }
    await connected();
    let releaseFiles;
    state.commitFilesGate = new Promise(resolve => { releaseFiles = resolve; });
    try {
      await page.locator('.commit-row').first().click();
      await page.locator('.commit-changes [role="status"]').waitFor();
      await connected();
      releaseFiles(); state.commitFilesGate = null;
      await page.locator('.commit-changes .change-row').waitFor();
      await connected();
      await page.getByRole('button', { name: 'Commits 视图选项', exact: true }).click();
      await page.getByRole('menuitemradio', { name: 'View as Tree', exact: true }).click();
      await page.locator('.commit-changes .source-change-tree').waitFor();
      await connected();
      if (process.env.AOW_GIT_GRAPH_SCREENSHOT) await page.locator('.source-control').screenshot({ path: process.env.AOW_GIT_GRAPH_SCREENSHOT });
      await page.locator('.commit-changes .change-folder-row').first().click();
      await page.locator('.commit-changes .change-row').waitFor({ state: 'detached' });
      await connected();
      state.commitFiles = [];
      await page.locator('.commit-row').nth(1).click();
      await page.locator('.commit-changes .side-empty').filter({ hasText: '此提交没有文件变更' }).waitFor();
      await connected();
      await page.locator('.commit-row').first().click();
      await connected();
    } finally { releaseFiles(); state.commitFilesGate = null; }
  });

  await test('Source Control menus dismiss outside, toggle, and preserve independent layouts', async t => {
    const { page } = await fixture(t);
    const changes = page.getByRole('button', { name: 'Changes 视图选项', exact: true });
    const commitsMenu = page.getByRole('button', { name: 'Commits 视图选项', exact: true });
    const menu = page.locator('.layout-menu');
    assert.equal(await changes.locator('svg.lucide-ellipsis').count(), 1);
    assert.equal(await commitsMenu.locator('svg.lucide-ellipsis').count(), 1);
    await changes.click();
    assert.deepEqual(await menu.locator('button').allTextContents(), [' View as list', ' View as Tree']);
    await changes.click();
    await menu.waitFor({ state: 'detached' });
    await changes.click();
    // Capture dismissal also works when another component stops propagation.
    await page.locator('[data-pins]').evaluate(element => element.addEventListener('pointerdown', event => event.stopPropagation()));
    await page.locator('[data-pins]').click();
    await menu.waitFor({ state: 'detached' });
    await changes.click();
    await commitsMenu.click();
    assert.deepEqual(await menu.locator('button').allTextContents(), [' View as list', ' View as Tree', ' git pull', ' git push']);
    await menu.getByRole('separator').waitFor();
    await page.keyboard.press('Escape');
    await menu.waitFor({ state: 'detached' });
    assert.equal(await commitsMenu.evaluate(element => element === document.activeElement), true);
    await changes.click();
    await menu.getByRole('menuitemradio', { name: 'View as Tree', exact: true }).click();
    await page.locator('.changes-list .source-change-tree').waitFor();
    await navigate(page, 'Commits');
    await page.locator('.commit-row').first().click();
    await page.waitForFunction(() => document.querySelector('.commit-changes .source-change-list'));
    await navigate(page, 'Commits');
    await page.locator('.commit-changes .change-row').first().waitFor();
    assert.equal(await page.locator('.commit-changes .source-change-tree').count(), 0);
    await commitsMenu.click();
    await menu.getByRole('menuitemradio', { name: 'View as Tree', exact: true }).click();
    await page.locator('.commit-changes .source-change-tree').waitFor();
    await changes.click();
    await menu.getByRole('menuitemradio', { name: 'View as list', exact: true }).click();
    assert.equal(await page.locator('.changes-list .source-change-tree').count(), 0);
    assert.equal(await page.locator('.commit-changes .source-change-tree').count(), 1);
    await commitsMenu.click();
    await navigate(page, 'Branches');
    await page.locator('.branch-label').click();
    await menu.waitFor({ state: 'detached' });
    await commitsMenu.click();
    await page.evaluate(() => window.performancePreview.setVisible(false));
    // Let React commit the hidden state before toggling it back on.
    await page.locator('.source-control').waitFor({ state: 'hidden' });
    await menu.waitFor({ state: 'detached' });
    await page.evaluate(() => window.performancePreview.setVisible(true));
    await menu.waitFor({ state: 'detached' });
  });

  await test('git commands run once in the selected repo, disable duplicates, refresh and retain errors', async t => {
    const { page, state } = await fixture(t);
    const menuButton = page.getByRole('button', { name: 'Commits 视图选项', exact: true });
    let release;
    state.commandGate = new Promise(resolve => { release = resolve; });
    const before = state.gitRequests.length;
    try {
      await menuButton.click();
      await page.getByRole('menuitem', { name: 'git pull', exact: true }).click();
      await page.getByRole('status').filter({ hasText: 'git pull 执行中' }).waitFor();
      await page.locator('.layout-menu').waitFor({ state: 'detached' });
      await menuButton.click();
      assert.equal(await page.getByRole('menuitem', { name: 'git pull', exact: true }).isDisabled(), true);
      assert.equal(await page.getByRole('menuitem', { name: 'git push', exact: true }).isDisabled(), true);
      // Remote operations may take longer than the normal 12-second API timeout.
      await page.clock.runFor(15000);
      assert.equal(await page.locator('.source-control').getByRole('status').textContent(), 'git pull 执行中…');
      assert.deepEqual(state.gitCommands, [{ command: 'pull', method: 'POST', repo: '/fixture' }]);
      state.branch = 'after-pull';
    } finally { release(); state.commandGate = null; }
    await page.getByRole('status').filter({ hasText: 'git pull 已完成' }).waitFor();
    await page.locator('.branch-name').filter({ hasText: 'after-pull' }).waitFor();
    assert.ok(state.gitRequests.length >= before + 2);
    state.commandError = 'Push rejected: fetch first';
    await page.getByRole('menuitem', { name: 'git push', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: state.commandError }).waitFor();
    await page.clock.runFor(5100);
    assert.equal(await page.getByRole('alert').textContent(), `git push 失败：${state.commandError}`);
    assert.deepEqual(state.gitCommands.at(-1), { command: 'push', method: 'POST', repo: '/fixture' });
    state.commandError = '';
    state.abortCommand = true;
    await menuButton.click();
    await page.getByRole('menuitem', { name: 'git push', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Failed to fetch' }).waitFor();
    await page.clock.runFor(1000);
    assert.equal(state.gitCommands.length, 3, 'network failures must not retry a mutating command');
  });

  await test('Source Control polling preserves expanded commits, selection and scroll; partial failures recover', async t => {
    const { page, state } = await fixture(t);
    await navigate(page, 'Commits');
    await page.locator('.commit-row').first().click();
    await page.waitForFunction(() => document.querySelector('.commit-changes .source-change-list'));
    await navigate(page, 'Commits');
    await page.locator('.commit-changes .change-row').first().waitFor();
    const list = page.locator('.changes-list');
    const scroller = page.locator('.aow-panel-viewport');
    await navigate(page, 'Changes');
    await list.locator('.change-row').first().press('End');
    await page.keyboard.press('Enter');
    const selected = await list.locator('[aria-current="true"]').getAttribute('title');
    const scrollTop = await scroller.evaluate(element => element.scrollTop);
    const counts = state.gitRequests.length;
    state.branch = 'updated';
    await page.clock.runFor(5100);
    await page.locator('.branch-name').filter({ hasText: 'updated' }).waitFor();
    assert.equal(state.gitRequests.length, counts + 2);
    assert.equal(await page.locator('.commit-row').first().getAttribute('aria-expanded'), 'true');
    assert.equal(await list.locator('[aria-current="true"]').getAttribute('title'), selected);
    assert.equal(await scroller.evaluate(element => element.scrollTop), scrollTop);
    state.failStatus = true;
    state.commits = [...state.commits, { ...commits[1], id: 'commit-2', subject: 'New commit' }];
    await page.clock.runFor(5100);
    await page.locator('.side-error').filter({ hasText: 'Status unavailable' }).waitFor();
    assert.equal(await page.locator('.commit-row').count(), 3);
    assert.equal(await list.locator('[aria-current="true"]').getAttribute('title'), selected);
    state.failStatus = false;
    await page.clock.runFor(5100);
    await page.locator('.side-error').waitFor({ state: 'detached' });
  });

  await test('slow Source Control rounds serialize manual refreshes and wait five seconds after completion', async t => {
    const { page, state } = await fixture(t);
    let release;
    state.statusGate = new Promise(resolve => { release = resolve; });
    try {
      const before = state.gitRequests.length;
      await page.getByRole('button', { name: '刷新 Changes', exact: true }).click();
      await delay(100);
      assert.equal(state.gitRequests.length, before + 1);
      await page.clock.runFor(6000);
      assert.equal(state.gitRequests.length, before + 1, 'no timer tick during a slow request');
      for (let i = 0; i < 3; i++) await page.getByRole('button', { name: '刷新 Changes', exact: true }).click();
      await page.getByRole('button', { name: '刷新 Commits', exact: true }).click();
      assert.equal(state.gitRequests.length, before + 1, 'manual refreshes share the in-flight round');
      release(); state.statusGate = null;
      await delay(250);
      assert.equal(state.gitRequests.length, before + 3, 'pending requests merge into one round');
      await page.clock.runFor(4000);
      assert.equal(state.gitRequests.length, before + 3);
      await page.clock.runFor(1200);
      await delay(150);
      assert.equal(state.gitRequests.length, before + 5);
    } finally { release(); state.statusGate = null; }
  });

  await test('hidden Source Control cancels requests, rejects stale data, and resumes immediately', async t => {
    const { page, state } = await fixture(t);
    let release;
    state.branch = 'stale';
    state.statusGate = new Promise(resolve => { release = resolve; });
    try {
      const aborted = page.waitForEvent('requestfailed', request => request.url().includes('/api/git/status'));
      await page.getByRole('button', { name: '刷新 Changes', exact: true }).click();
      await delay(50);
      await page.evaluate(() => window.performancePreview.setVisible(false));
      await aborted;
      const count = state.gitRequests.length;
      await page.clock.runFor(20000);
      assert.equal(state.gitRequests.length, count);
      release(); state.statusGate = null;
      state.branch = 'fresh';
      await page.evaluate(() => window.performancePreview.setVisible(true));
      await page.locator('.branch-name').filter({ hasText: 'fresh' }).waitFor();
      assert.equal(state.gitRequests.length, count + 2);
      await page.evaluate(() => {
        Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
        document.dispatchEvent(new Event('visibilitychange'));
      });
      await page.clock.runFor(20000);
      assert.equal(state.gitRequests.length, count + 2);
      await page.evaluate(() => {
        Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
        document.dispatchEvent(new Event('visibilitychange'));
      });
      await delay(150);
      assert.equal(state.gitRequests.length, count + 4);
      assert.equal(await page.locator('.side-error').count(), 0);
    } finally { release(); state.statusGate = null; }
  });

  await test('unchanged background pins do not rerender; ordered updates, errors and foreground loading remain observable', async t => {
    const { page, state } = await fixture(t);
    const renders = await page.evaluate(() => window.performancePreview.renders);
    const requests = state.pinRequests;
    await page.clock.runFor(10100);
    await page.waitForFunction(() => !document.querySelector('button').disabled);
    assert.ok(state.pinRequests > requests);
    assert.equal(await page.evaluate(() => window.performancePreview.renders), renders);
    state.paths = ['/b', '/a'];
    await page.clock.runFor(10100);
    await page.waitForFunction(() => document.querySelector('[data-pins]').textContent === '["/b","/a"]');
    state.failPins = true;
    await page.clock.runFor(10100);
    await page.locator('[data-error]').filter({ hasText: 'Fixture unavailable' }).waitFor();
    state.failPins = false;
    await page.clock.runFor(10100);
    await page.waitForFunction(() => document.querySelector('[data-error]').textContent === '');
    let release;
    state.pinGate = new Promise(resolve => { release = resolve; });
    try {
      await page.getByRole('button', { name: 'Refresh pins', exact: true }).click();
      assert.equal(await page.getByRole('button', { name: 'Refresh pins', exact: true }).isDisabled(), true);
    } finally { release(); state.pinGate = null; }
    await page.waitForFunction(() => !document.querySelector('button').disabled);
    state.pinGate = new Promise(resolve => { release = resolve; });
    try {
      await page.evaluate(() => window.dispatchEvent(new Event('focus')));
      assert.equal(await page.getByRole('button', { name: 'Refresh pins', exact: true }).isDisabled(), false);
      await page.evaluate(() => window.performancePreview.updatePins(previous => new Set([...previous, '/c'])));
      assert.equal(await page.getByRole('button', { name: 'Refresh pins', exact: true }).isDisabled(), true);
    } finally { release(); state.pinGate = null; }
    await page.waitForFunction(() => document.querySelector('[data-pins]').textContent === '["/b","/a","/c"]');
  });

  await test('large Changes lists bound DOM, preserve selection, tree collapse and scroll through hiding and unrelated updates', async t => {
    const { page } = await fixture(t);
    const list = page.locator('.changes-list');
    const scroller = page.locator('.aow-panel-viewport');
    await navigate(page, 'Changes');
    const mountedRows = await list.locator('.change-row').count();
    assert.ok(mountedRows < 100);
    t.diagnostic(`20,000 files: ${mountedRows} mounted change rows`);
    await list.locator('.change-row').first().click();
    assert.equal(await page.evaluate(() => window.performancePreview.opened.at(-1).staged), true);
    assert.equal(await list.locator('.change-row[aria-current="true"]').count(), 1);
    await list.locator('.change-row').nth(1).click();
    assert.equal(await page.evaluate(() => window.performancePreview.opened.at(-1).staged), false);
    await list.locator('.change-row').first().press('End');
    await list.locator('.change-row[title="dir-19/file-19999.ts · working tree"]').waitFor();
    assert.ok(await list.locator('.change-row').count() < 100);
    await page.keyboard.press('Enter');
    assert.equal(await page.evaluate(() => window.performancePreview.opened.at(-1).file.path), files.at(-1).path);
    const scrollTop = await scroller.evaluate(element => element.scrollTop);
    await page.evaluate(() => window.performancePreview.setVisible(false));
    assert.equal(await page.locator('.change-row').count(), 0);
    await page.evaluate(() => window.performancePreview.setVisible(true));
    await list.locator('.change-row').first().waitFor();
    assert.equal(await scroller.evaluate(element => element.scrollTop), scrollTop);
    await page.getByRole('button', { name: 'Changes 视图选项', exact: true }).click();
    await page.getByRole('menuitemradio', { name: 'View as Tree', exact: true }).click();
    await scroller.evaluate(element => { element.scrollTop = 0; });
    const folder = list.locator('.change-folder-row[title="dir-00"]');
    await folder.click();
    assert.equal(await folder.getAttribute('aria-expanded'), 'false');
    await page.evaluate(() => window.performancePreview.update());
    assert.equal(await folder.getAttribute('aria-expanded'), 'false');
    await page.getByRole('button', { name: /^(展开|收起) Changes$/ }).click();
    assert.equal(await list.locator('.change-row').count(), 0);
    await page.getByRole('button', { name: /^(展开|收起) Changes$/ }).click();
    await folder.waitFor();
    assert.equal(await folder.getAttribute('aria-expanded'), 'false');
    await folder.click();
    await list.locator('.change-row').first().waitFor();
    assert.ok(await page.locator('*').count() < 1500);
  });

  await test('expanded commit file lists share the commit viewport and keep correct targets after preceding lists collapse', async t => {
    const { page } = await fixture(t);
    const list = page.locator('.commit-list');
    await navigate(page, 'Commits');
    await list.locator('.commit-row').nth(0).dispatchEvent('click');
    await list.locator('.commit-row').nth(1).dispatchEvent('click');
    await page.waitForFunction(() => document.querySelectorAll('.commit-changes .source-change-list').length === 2);
    await list.locator('.change-row').first().waitFor();
    assert.ok(await list.locator('.change-row').count() < 100);
    await list.locator('.commit-row').nth(0).dispatchEvent('click');
    await navigate(page, 'Commits');
    const secondFiles = list.locator('.commit-node').nth(1).locator('.change-row');
    await secondFiles.first().waitFor();
    await secondFiles.first().press('End');
    await secondFiles.filter({ hasText: 'file-19999.ts' }).waitFor();
    await page.keyboard.press('Enter');
    assert.equal(await page.evaluate(() => window.performancePreview.opened.at(-1).commit), 'commit-1');
    await page.getByRole('button', { name: /^(展开|收起) Commits$/ }).click();
    assert.equal(await list.locator('.change-row').count(), 0);
    await page.getByRole('button', { name: /^(展开|收起) Commits$/ }).click();
    await navigate(page, 'Commits');
    await secondFiles.first().waitFor();
    assert.ok(await list.locator('.change-row').count() < 100);
  });
} finally {
  await browser?.close();
  await server.close();
}
