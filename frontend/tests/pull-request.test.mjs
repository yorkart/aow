import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { after, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import ts from 'typescript';

const source = await readFile(new URL('../src/features/pr/pullRequestPresentation.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { stateClass, stateLabel, safeExternalUrl, formatPrTime, parsePullRequestNumber } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);

test('CI conclusions distinguish waiting, failures, and unknown states', () => {
  for (const state of ['all_passed', 'succeeded', 'approved']) assert.equal(stateClass(state), 'passed');
  for (const state of ['some_failed', 'timed_out', 'operation_required', 'canceled']) assert.equal(stateClass(state), 'failed');
  for (const state of ['queued', 'in_progress', 'pending']) assert.equal(stateClass(state), 'pending');
  assert.equal(stateClass('future_status'), 'neutral');
  assert.equal(stateLabel('future_status'), 'future_status');
  assert.equal(stateLabel(''), '状态未知');
  assert.equal(stateClass('no_checks'), 'neutral');
});

test('external detail links only allow explicit HTTP(S) destinations', () => {
  assert.equal(safeExternalUrl('https://github.com/team/repo/pull/42'), 'https://github.com/team/repo/pull/42');
  for (const value of ['javascript:alert(1)', 'data:text/html,test', 'file:///tmp/test', '/relative', '', null]) assert.equal(safeExternalUrl(value), undefined);
});

test('missing or malformed dates do not show invalid dates or pretend to be current', () => {
  assert.equal(formatPrTime(null), '未提供');
  assert.equal(formatPrTime(''), '未提供');
  assert.equal(formatPrTime('bad date'), '未提供');
  assert.match(formatPrTime('2026-09-12T08:30:00Z'), /2026/);
});

test('mobile PR links accept only positive safe integer identifiers', () => {
  assert.equal(parsePullRequestNumber('3102'), 3102);
  for (const value of [undefined, '', '0', '-1', '1.5', '1e2', 'NaN', 'Infinity', '9007199254740992', '../42']) assert.equal(parsePullRequestNumber(value), undefined);
});

const diffSource = await readFile(new URL('../src/features/pr/pullRequestDiff.ts', import.meta.url), 'utf8');
const diffModule = ts.transpileModule(diffSource, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } });
const { pullRequestDiffLines } = await import(`data:text/javascript;base64,${Buffer.from(diffModule.outputText).toString('base64')}`);

test('mobile patch line numbers follow multiple hunks and no-newline markers', () => {
  const lines = pullRequestDiffLines('--- a/file\n+++ b/file\n@@ -8,2 +8,2 @@\n context\n-removed\n+added\n\\ No newline at end of file\n@@ -20 +30,2 @@\n-old\n+++new\n+tail\n');
  assert.equal(lines[1].kind, 'meta');
  assert.deepEqual(lines[3], { kind: 'context', text: 'context', oldLine: 8, newLine: 8 });
  assert.deepEqual(lines[4], { kind: 'removed', text: 'removed', oldLine: 9 });
  assert.deepEqual(lines[5], { kind: 'added', text: 'added', newLine: 9 });
  assert.equal(lines[6].kind, 'meta');
  assert.deepEqual(lines[8], { kind: 'removed', text: 'old', oldLine: 20 });
  assert.deepEqual(lines[9], { kind: 'added', text: '++new', newLine: 30 });
  assert.deepEqual(lines[10], { kind: 'added', text: 'tail', newLine: 31 });
});

test('mobile patch handles newly added and deleted files', () => {
  assert.deepEqual(pullRequestDiffLines(''), []);
  assert.deepEqual(pullRequestDiffLines('@@ -0,0 +1 @@\n+new\n')[1], { kind: 'added', text: 'new', newLine: 1 });
  assert.deepEqual(pullRequestDiffLines('@@ -1 +0,0 @@\n-deleted\n')[1], { kind: 'removed', text: 'deleted', oldLine: 1 });
});

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: null } });
after(() => server.close());
const { prApi: api } = await server.ssrLoadModule('/src/features/pr/api.ts');

test('review API preserves provider and remote on list, detail and diff with a matching timeout', async t => {
  const requests = [], timeouts = [];
  t.mock.method(globalThis, 'fetch', async url => { requests.push(new URL(url, 'http://aow')); return new Response('{}', { headers: { 'Content-Type': 'application/json' } }); });
  const previousWindow = globalThis.window;
  globalThis.window = { setTimeout: (fn, delay) => { timeouts.push(delay); return setTimeout(fn, delay); }, clearTimeout };
  t.after(() => { globalThis.window = previousWindow; });
  const target = { provider: 'custom', remote: 'upstream' };
  await api.myPullRequests('/repo space', target);
  await api.myPullRequest('/repo space', 42, target);
  await api.myPullRequestDiff('/repo space', 42, 'src/file #中文.py', true, target);
  await api.myPullRequests('/repo space', target, 'all');
  for (const url of requests) {
    assert.equal(url.searchParams.get('repo'), '/repo space');
    assert.equal(url.searchParams.get('provider'), 'custom');
    assert.equal(url.searchParams.get('remote'), 'upstream');
  }
  assert.equal(requests[2].searchParams.get('path'), 'src/file #中文.py');
  assert.equal(requests[2].searchParams.get('patch_only'), 'true');
  assert.equal(requests[0].searchParams.get('state'), null);
  assert.equal(requests[3].searchParams.get('state'), 'all');
  assert.deepEqual(timeouts, [65000, 65000, 65000, 65000]);
});

test('review scripts are not silently retried when transport fails', async t => {
  let calls = 0;
  t.mock.method(globalThis, 'fetch', async () => { calls++; throw new Error('connection lost'); });
  const previousWindow = globalThis.window;
  globalThis.window = { setTimeout, clearTimeout };
  t.after(() => { globalThis.window = previousWindow; });
  await assert.rejects(api.myPullRequests('/repo'), /connection lost/);
  assert.equal(calls, 1);
});

test('desktop review diffs release models after detaching and reopen without stale content', async t => {
  const { chromium } = await import('playwright');
  const { detail } = await import('./fixtures/pull-request.mjs');
  await server.listen();
  const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  t.after(() => browser.close());
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  let revision = 1;
  await page.route('**/api/**', route => {
    const url = new URL(route.request().url());
    return route.fulfill({ json: url.pathname.endsWith('/diff')
      ? { original: 'before\n', modified: `after ${revision}\n`, binary: false, truncated: false }
      : { ...detail, files: detail.files.slice(0, 2) } });
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/pull-request-preview.html`);
  await page.getByRole('tab', { name: /文件变更/ }).click();
  const file = page.locator('.pr-file-header').first();
  const models = async () => page.evaluate(async () => {
    const { monaco } = await import('/src/features/editor/monaco.ts');
    return monaco.editor.getModels().filter(model => model.uri.authority === 'pull-request').map(model => model.getValue()).sort();
  });
  const waitForModels = expected => page.waitForFunction(async expected => {
    const { monaco } = await import('/src/features/editor/monaco.ts');
    return monaco.editor.getModels().filter(model => model.uri.authority === 'pull-request').length === expected;
  }, expected);
  for (let round = 0; round < 2; round++) {
    await file.click();
    await page.locator('.monaco-diff-editor').waitFor();
    assert.deepEqual(await models(), ['after 1\n', 'before\n']);
    // Filtering unmounts the diff; clearing restores the still-expanded file.
    await page.getByRole('textbox', { name: '筛选变更文件' }).fill('no-matching-file');
    await waitForModels(0);
    await page.getByRole('button', { name: '清除文件筛选' }).click();
    await waitForModels(2);
    assert.deepEqual(await models(), ['after 1\n', 'before\n']);
    await file.click();
    await waitForModels(0);
  }
  revision = 2;
  await page.getByRole('button', { name: '刷新 PR 详情' }).click();
  await file.click();
  await waitForModels(2);
  assert.deepEqual(await models(), ['after 2\n', 'before\n']);
  await page.getByRole('button', { name: '收起全部' }).click();
  await waitForModels(0);
  assert.deepEqual(errors, []);
});

async function panelFixture(t, { fail = false } = {}) {
  const { chromium } = await import('playwright');
  const { detail, pullRequests } = await import('./fixtures/pull-request.mjs');
  if (!server.httpServer?.listening) await server.listen();
  const browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const page = await browser.newPage({ viewport: { width: 1100, height: 720 }, reducedMotion: 'reduce' });
  page.setDefaultTimeout(10000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  t.after(async () => { await browser.close(); assert.deepEqual(errors, []); });
  const state = { items: structuredClone(pullRequests), fail, queries: [] };
  await page.route('**/api/**', route => {
    const url = new URL(route.request().url());
    if (url.pathname === '/api/review-targets') return route.fulfill({ json: [
      { provider: 'github', provider_name: 'GitHub', remote: 'origin', host: 'github.com', repository: 'example/aow' },
    ] });
    if (url.pathname === '/api/my-pull-requests') {
      state.queries.push(url.searchParams);
      if (state.fail) return route.fulfill({ status: 503, json: { message: 'PR provider unavailable' } });
      return route.fulfill({ json: { repository: '/fixtures/default', current_branch: detail.source_branch,
        current_user: detail.author, pull_requests: state.items.map(pr => ({ ...pr, provider: 'github', remote: 'origin' })) } });
    }
    const pr = state.items.find(pr => url.pathname === `/api/my-pull-requests/${pr.number}`);
    return pr ? route.fulfill({ json: pr }) : route.fulfill({ status: 404, json: { message: 'Missing PR fixture' } });
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/pull-request-preview.html?panel`);
  const panel = title => page.getByRole('region', { name: title, exact: true });
  const rows = title => panel(title).locator('.aow-list-row-title');
  return { page, panel, rows, state };
}

test('PR panels group by status, sort recent updates first and open finished PR details', async t => {
  const { page, panel, rows, state } = await panelFixture(t);
  await rows('Closed PRs').waitFor();
  assert.deepEqual(await page.locator('.pull-requests-pr-section').evaluateAll(elements => elements.map(element => element.getAttribute('aria-label'))), ['Open PRs', 'Merged PRs', 'Closed PRs']);
  assert.deepEqual((await rows('Open PRs').allTextContents()).map(title => title.match(/#\d+/)[0]), ['#43', '#42']);
  assert.match(await rows('Merged PRs').textContent(), /#44/);
  assert.match(await rows('Closed PRs').textContent(), /#45/);
  assert.ok(state.queries.length > 0);
  for (const query of state.queries) {
    assert.equal(query.get('state'), 'all');
    assert.equal(query.get('provider'), 'github');
    assert.equal(query.get('remote'), 'origin');
  }
  if (process.env.AOW_PR_PANEL_SCREENSHOT) await page.locator('aside').screenshot({ path: process.env.AOW_PR_PANEL_SCREENSHOT });
  for (const [title, number] of [['Merged PRs', 44], ['Closed PRs', 45]]) {
    await panel(title).locator('.aow-list-row-open').click();
    await page.getByRole('article', { name: `PR #${number} 详情`, exact: true }).waitFor();
    assert.equal(await panel(title).locator('.my-pr-row.selected').count(), 1);
  }

  await panel('Open PRs').getByRole('button', { name: '收起 Open PRs', exact: true }).click();
  state.items = state.items.map(pr => pr.number === 42 ? { ...pr, status: 'merged', updated_at: '2026-09-26T03:45:00Z' } : pr);
  await panel('Merged PRs').getByRole('button', { name: '刷新 Merged PRs', exact: true }).click();
  await panel('Merged PRs').getByText(/#42 ·/).waitFor();
  assert.equal(await panel('Open PRs').getByRole('button', { name: '展开 Open PRs', exact: true }).getAttribute('aria-expanded'), 'false');
  assert.deepEqual((await rows('Merged PRs').allTextContents()).map(title => title.match(/#\d+/)[0]), ['#42', '#44']);
  await panel('Open PRs').getByRole('button', { name: '展开 Open PRs', exact: true }).click();
  assert.equal(await rows('Open PRs').count(), 1);
});

test('Open PR menu only filters drafts, supports dismissal and remembers the preference', async t => {
  const { page, panel, rows, state } = await panelFixture(t);
  await rows('Closed PRs').waitFor();
  const trigger = panel('Open PRs').getByRole('button', { name: 'Open PRs 过滤选项', exact: true });
  assert.equal(await panel('Open PRs').locator('.aow-panel-header-actions > button').last().getAttribute('aria-label'), 'Open PRs 过滤选项');
  const menu = page.getByRole('menu', { name: 'Open PRs 过滤选项', exact: true });
  const checkbox = menu.getByRole('menuitemcheckbox', { name: '过滤 Draft', exact: true });
  const requestsBeforeFilter = state.queries.length;
  await trigger.click();
  assert.equal(await menu.locator('button').count(), 1);
  assert.equal(await checkbox.getAttribute('aria-checked'), 'false');
  await checkbox.click();
  await menu.waitFor({ state: 'hidden' });
  assert.equal(await rows('Open PRs').count(), 1);
  assert.match(await rows('Open PRs').textContent(), /#42/);
  assert.match(await rows('Closed PRs').textContent(), /Draft #45/);
  assert.equal(await rows('Merged PRs').count(), 1);
  assert.equal(state.queries.length, requestsBeforeFilter);

  await trigger.press('Enter');
  assert.equal(await checkbox.getAttribute('aria-checked'), 'true');
  await page.keyboard.press('Escape');
  await menu.waitFor({ state: 'hidden' });
  assert.equal(await trigger.evaluate(element => element === document.activeElement), true);
  await trigger.click();
  await page.locator('main').click();
  await menu.waitFor({ state: 'hidden' });

  await page.reload();
  await rows('Closed PRs').waitFor();
  assert.equal(await rows('Open PRs').count(), 1);
  await trigger.click();
  assert.equal(await checkbox.getAttribute('aria-checked'), 'true');
  await checkbox.click();
  assert.equal(await rows('Open PRs').count(), 2);
});

test('all three PR panels recover from errors and retain accessible empty headers', async t => {
  const { page, panel, rows, state } = await panelFixture(t, { fail: true });
  await panel('Open PRs').getByRole('alert').waitFor();
  for (const title of ['Open PRs', 'Merged PRs', 'Closed PRs']) {
    assert.match(await panel(title).getByRole('alert').textContent(), /PR provider unavailable/);
  }
  state.fail = false;
  state.items = [];
  await panel('Closed PRs').getByRole('button', { name: '刷新 Closed PRs', exact: true }).click();
  await page.getByText('@chen.yu · 当前用户', { exact: true }).waitFor();
  for (const title of ['Open PRs', 'Merged PRs', 'Closed PRs']) {
    assert.equal(await rows(title).count(), 0);
    await panel(title).getByRole('button', { name: `展开 ${title}`, exact: true }).click();
    await panel(title).getByText(`当前仓库没有该用户创建的 ${title}。`, { exact: true }).waitFor();
  }
});
