import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  async function fixture(t, width = 1200) {
    const page = await browser.newPage({ viewport: { width, height: 850 } });
    page.setDefaultTimeout(10000);
    t.after(() => page.close());
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await installLiveEvents(page, { boot_id: 'sources', revision: 0, projects: 0, terminals: 0, tasks: 0, repositories: {} });
    const configs = Object.fromEntries(['project', 'other'].map(project => [project, { revision: 0, sources: [{ type: 'inbox' }, { type: 'repository_issues', enabled: true, provider: null, remote: null }] }]));
    const makeIssue = (number, title, labels, status = 'open') => ({ number, title, labels, status, assignees: ['alice'], url: `https://example.com/issues/${number}`, updated_at: '2026-09-29T00:00:00Z' });
    const items = [makeIssue(1, '修复共享需求的加载问题', ['bug']), makeIssue(2, '支持团队共享需求', ['feature', '需求, UI']), makeIssue(3, '完善来源说明', [])];
    const queries = []; let failure = false; let conflict = false; let held;
    await page.route('**/api/tasks**', async route => {
      const req = route.request(); const url = new URL(req.url()); const parts = url.pathname.split('/');
      if (parts[3] !== 'sources') return route.fulfill({ json: parts[3] === 'inbox' ? { items: [], total: 0, next_cursor: null } : { version: 1, tasks: [], statuses: [], status_revision: 1 } });
      const project = parts[4]; const operation = parts[5];
      if (!operation) {
        if (req.method() === 'PUT') {
          if (conflict) return route.fulfill({ status: 409, json: { message: '配置已更新，请重新加载后保存。' } });
          configs[project] = { ...req.postDataJSON(), revision: configs[project].revision + 1 };
        }
        return route.fulfill({ json: configs[project] });
      }
      if (operation === 'targets') return route.fulfill({ json: { repository: `/${project}`, targets: [{ provider: 'github', provider_name: 'GitHub', remote: 'origin', host: 'github.com', repository: 'team/repo' }, { provider: 'github', provider_name: 'GitHub', remote: 'upstream', host: 'github.com', repository: 'team/upstream' }] } });
      if (operation === 'labels') return route.fulfill({ json: { labels: ['bug', 'feature', '需求, UI', 'empty'].map(name => ({ name, color: '62b58d', description: name })) } });
      const input = req.postDataJSON(); queries.push({ project, ...input });
      if (failure) return route.fulfill({ status: 502, json: { message: 'Provider 登录已过期，请重新登录' } });
      const selected = project === 'other' ? [makeIssue(10, 'Other 仓库需求', [])] : items.filter(issue => (!input.labels.length || input.labels.some(label => issue.labels.includes(label))) && (input.state === 'all' || issue.status === input.state));
      const fulfill = () => route.fulfill({ json: { issues: selected, provider: 'github', provider_name: 'GitHub', remote: configs[project].sources[1].remote ?? 'origin' } }).catch(() => {});
      if (held && input.labels.includes('bug')) { const next = held; held = undefined; next(fulfill); return; }
      await fulfill();
    });
    await page.goto(`${base}/tests/issue-sources-preview.html`);
    await page.getByRole('link', { name: '支持团队共享需求' }).waitFor();
    return { page, configs, queries, errors, fail: value => { failure = value; }, conflict: value => { conflict = value; }, hold: () => new Promise(resolve => { held = resolve; }) };
  }

  await test('Issue appears below Inbox, multiple labels match OR, filters clear and links open the original', async t => {
    const { page, queries, errors } = await fixture(t);
    const inbox = await page.getByRole('region', { name: 'Inbox', exact: true }).boundingBox();
    const issue = await page.getByRole('region', { name: 'Issue', exact: true }).boundingBox();
    assert.ok(issue.y >= inbox.y + inbox.height - 1);
    await page.getByRole('button', { name: '按 Label 筛选 Issue' }).click();
    await page.getByRole('checkbox', { name: 'bug', exact: true }).check();
    await page.getByRole('link', { name: '修复共享需求的加载问题' }).waitFor();
    assert.equal(await page.getByRole('link', { name: '支持团队共享需求' }).count(), 0);
    await page.getByRole('checkbox', { name: '需求, UI', exact: true }).check();
    await page.getByRole('link', { name: '支持团队共享需求' }).waitFor();
    assert.deepEqual(queries.at(-1).labels, ['bug', '需求, UI']);
    const link = page.getByRole('link', { name: '支持团队共享需求' });
    assert.equal(await link.getAttribute('href'), 'https://example.com/issues/2');
    assert.equal(await link.getAttribute('target'), '_blank');
    await page.screenshot({ path: '/tmp/aow-issue-desktop.png' });
    await page.getByRole('button', { name: '清除筛选' }).click();
    await page.getByRole('link', { name: '完善来源说明' }).waitFor();
    await page.getByRole('checkbox', { name: 'empty', exact: true }).check();
    await page.getByText('没有匹配所选 Label 的 Issue').waitFor();
    await page.getByRole('button', { name: '清除筛选' }).click();
    await page.getByLabel('Issue 状态').selectOption('closed');
    await page.getByText('暂无符合条件的 Issue').waitFor();
    assert.equal(queries.at(-1).state, 'closed');
    assert.deepEqual(errors, []);
  });

  await test('source settings persist per project, handle conflicts and enable/disable without querying disabled sources', async t => {
    const { page, configs, queries, errors, conflict } = await fixture(t, 390);
    await page.getByRole('button', { name: '需求源配置', exact: true }).click();
    let dialog = page.getByRole('dialog', { name: '需求源配置' });
    await dialog.getByLabel('Issue 关联仓库').selectOption(JSON.stringify(['github', 'upstream']));
    await page.screenshot({ path: '/tmp/aow-issue-source-settings.png' });
    conflict(true);
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').waitFor();
    assert.equal(configs.project.revision, 0);
    conflict(false);
    await dialog.getByLabel('启用 Issue 需求源').uncheck();
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('Issue 需求源已停用', { exact: true }).waitFor();
    assert.equal(configs.project.sources[1].remote, 'upstream');
    const count = queries.length;
    await page.reload();
    await page.getByText('Issue 需求源已停用', { exact: true }).waitFor();
    assert.equal(queries.length, count);
    await page.getByRole('button', { name: '配置需求源', exact: true }).click();
    dialog = page.getByRole('dialog', { name: '需求源配置' });
    assert.equal(await dialog.getByLabel('Issue 关联仓库').inputValue(), JSON.stringify(['github', 'upstream']));
    await dialog.getByLabel('启用 Issue 需求源').check();
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('GitHub · upstream', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'Other 项目', exact: true }).click();
    await page.getByRole('link', { name: 'Other 仓库需求' }).waitFor();
    assert.equal(await page.getByRole('link', { name: '支持团队共享需求' }).count(), 0);
    assert.equal(configs.other.revision, 0);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    await page.screenshot({ path: '/tmp/aow-issue-mobile.png' });
    assert.deepEqual(errors, []);
  });

  await test('late filter responses cannot replace the current selection; provider errors support retry', async t => {
    const { page, errors, fail, hold } = await fixture(t);
    await page.getByRole('button', { name: '按 Label 筛选 Issue' }).click();
    const pending = hold();
    await page.getByRole('checkbox', { name: 'bug', exact: true }).check();
    const release = await pending;
    await page.getByRole('button', { name: '清除筛选' }).click();
    await page.getByRole('link', { name: '完善来源说明' }).waitFor();
    await release();
    assert.equal(await page.getByRole('link', { name: '完善来源说明' }).count(), 1);
    fail(true);
    await page.getByRole('button', { name: '刷新 Issue', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: 'Provider 登录已过期' }).waitFor();
    assert.equal(await page.getByText('暂无符合条件的 Issue', { exact: true }).count(), 0);
    fail(false);
    await page.getByRole('button', { name: '重试', exact: true }).click();
    await page.getByRole('link', { name: '完善来源说明' }).waitFor();
    assert.deepEqual(errors, []);
  });
} finally { await browser?.close(); await server.close(); }
