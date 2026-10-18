import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
const pane = (id, parent, cli = true) => ({ id, parent: parent ? { kind: 'terminal', id: parent } : null, name: id, cwd: '/repo', shell: '/bin/sh', status: 'running',
  ...(cli ? { agent_id: 'codex', agent_terminal: { phase: 'ready', task_submitted: false, error: null } } : {}) });
const tab = (id, name, path, panes) => ({ id, name, workspace_root: path, panes, layout: { type: 'pane', pane_id: panes[0].id } });
const data = () => [
  tab('a', '实现登录功能', '/repo', [pane('pa', undefined, false)]),
  tab('b', '实现认证接口', '/auth', [pane('pb', 'pa')]),
  tab('c', '检查测试结果', '/tests', [pane('pc', 'pb')]),
  tab('d', '开发服务器', '/repo', [pane('pd', undefined, false)]),
];
const root = (page, id) => page.locator(`.terminal-family[data-tab-id="${id}"]`);
const expandRoot = (page, id) => root(page, id).locator(':scope > .terminal-family-root > .terminal-children-toggle').click();
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function fixture(t, tabs = data(), instances = []) {
    const page = await browser.newPage({ viewport: { width: 900, height: 700 } });
    page.setDefaultTimeout(8000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    await page.route(url => url.pathname === '/api/instances', route => route.fulfill({ json: { items: instances } }));
    await page.route(url => url.pathname === '/api/terminals', route => route.fulfill({ json: { tabs } }));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/terminal-hierarchy-preview.html`);
    await root(page, 'a').waitFor();
    return { page, update(next) { tabs = next; } };
  }

  await test('all original items retain status; each independently expands its descendants across groups and worktrees', async t => {
    const { page } = await fixture(t);
    assert.equal(await page.getByRole('region', { name: 'User Terminals', exact: true }).count(), 1);
    assert.equal(await page.getByRole('region', { name: 'CLI Terminals', exact: true }).count(), 1);
    assert.equal(await page.locator('.terminal-family').count(), 4);
    assert.equal(await page.locator('.terminal-panel-status').count(), 4);
    assert.equal(await page.locator('.terminal-descendant').count(), 0);
    assert.equal(await root(page, 'd').locator('.terminal-children-toggle').count(), 0);
    const before = await root(page, 'a').locator('.terminal-panel-row').boundingBox();
    await expandRoot(page, 'a');
    assert.equal(await root(page, 'a').locator('.terminal-descendant-open').innerText(), 'feat/auth · 实现认证接口');
    assert.equal(await root(page, 'a').locator('.terminal-descendant .terminal-panel-status').count(), 0);
    await root(page, 'a').getByRole('button', { name: '展开 feat/auth · 实现认证接口 的子实例', exact: true }).click();
    await expandRoot(page, 'b');
    assert.equal(await root(page, 'a').locator('[data-pane-id="pc"]').count(), 1);
    assert.equal(await root(page, 'b').locator('[data-pane-id="pc"]').count(), 1);
    assert.equal(await root(page, 'a').locator('.terminal-panel-row').evaluate(el => el.getBoundingClientRect().height), before.height);
    await root(page, 'a').getByRole('button', { name: 'feat/tests · 检查测试结果', exact: true }).click();
    assert.equal(await page.getByLabel('Opened pane').textContent(), 'c:pc');
    await page.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    assert.equal(await root(page, 'a').locator('[data-pane-id="pc"]').count(), 1);
    await page.locator('.terminal-panel').screenshot({ path: '/tmp/aow-terminal-hierarchy.png' });
    await expandRoot(page, 'a');
    assert.equal(await root(page, 'b').locator('[data-pane-id="pc"]').isVisible(), true);
  });

  await test('root filtering leaves complete descendant branches available, including on narrow screens', async t => {
    const { page } = await fixture(t);
    await page.getByRole('button', { name: '当前 worktree', exact: true }).click();
    assert.equal(await page.locator('.terminal-family').count(), 2);
    await expandRoot(page, 'a');
    await root(page, 'a').getByRole('button', { name: '展开 feat/auth · 实现认证接口 的子实例', exact: true }).click();
    await page.setViewportSize({ width: 360, height: 700 });
    await root(page, 'a').getByRole('button', { name: 'feat/tests · 检查测试结果', exact: true }).click();
    assert.equal(await page.getByLabel('Opened pane').textContent(), 'c:pc');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
  });

  await test('mixed ACP and terminal descendants retain deleted intermediate nodes', async t => {
    const item = (kind, id, parent, title, deleted = false) => ({ kind, id, parent, title, cwd: '/repo', agent_id: 'test', deleted,
      url: deleted ? null : `/aow/tabs/${kind}/${id}` });
    const instances = [
      item('acp', 'thread-one', { kind: 'terminal', id: 'pa' }, '分析任务'),
      item('terminal', 'removed-pane', { kind: 'acp', id: 'thread-one' }, '中间任务', true),
      item('acp', 'thread-two', { kind: 'terminal', id: 'removed-pane' }, '验证结果'),
    ];
    const { page } = await fixture(t, data(), instances);
    await expandRoot(page, 'a');
    const branch = root(page, 'a');
    await branch.getByRole('button', { name: '展开 main · ACP · 分析任务 的子实例', exact: true }).click();
    assert.equal(await branch.getByRole('button', { name: 'main · 中间任务（已删除）', exact: true }).isDisabled(), true);
    await branch.getByRole('button', { name: '展开 main · 中间任务（已删除） 的子实例', exact: true }).click();
    assert.equal(await branch.getByRole('button', { name: 'main · ACP · 验证结果', exact: true }).isVisible(), true);
    assert.equal(await branch.locator('[data-instance="acp:thread-two"]').count(), 1);
    await branch.getByRole('button', { name: 'main · ACP · 验证结果', exact: true }).click();
    assert.equal(await page.getByLabel('Opened pane').textContent(), 'acp:thread-two');
    instances[2].title = '已更新的验证结果';
    const refreshed = page.waitForResponse(response => new URL(response.url()).pathname === '/api/instances');
    await page.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await refreshed;
    await branch.getByRole('button', { name: 'main · ACP · 已更新的验证结果', exact: true }).waitFor();
  });

  await test('split-pane parents, missing parents and malformed cycles do not lose roots or recurse forever', async t => {
    const tabs = data();
    tabs[0].panes.push(pane('pa2', undefined, false));
    tabs[1].panes[0].parent = { kind: 'terminal', id: 'pa2' };
    tabs[0].panes[0].parent = { kind: 'terminal', id: 'pc' };
    tabs[3].panes[0].parent = { kind: 'terminal', id: 'missing' };
    const { page } = await fixture(t, tabs);
    await expandRoot(page, 'a');
    await root(page, 'a').getByRole('button', { name: '展开 feat/auth · 实现认证接口 的子实例', exact: true }).click();
    assert.equal(await root(page, 'a').locator('.terminal-descendant').count(), 2);
    assert.equal(await root(page, 'a').locator('[data-pane-id="pc"] .terminal-children-toggle').count(), 0);
    assert.equal(await root(page, 'd').isVisible(), true);
  });
} finally { await browser?.close(); await server.close(); }
