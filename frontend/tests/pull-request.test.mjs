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
  for (const url of requests) {
    assert.equal(url.searchParams.get('repo'), '/repo space');
    assert.equal(url.searchParams.get('provider'), 'custom');
    assert.equal(url.searchParams.get('remote'), 'upstream');
  }
  assert.equal(requests[2].searchParams.get('path'), 'src/file #中文.py');
  assert.equal(requests[2].searchParams.get('patch_only'), 'true');
  assert.deepEqual(timeouts, [65000, 65000, 65000]);
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
