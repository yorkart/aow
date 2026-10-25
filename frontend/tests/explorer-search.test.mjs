import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const harness = `<!doctype html><html><head><link rel="stylesheet" href="/src/styles.css"></head><body>
<div id="root"></div><script type="module">
import React from 'react';
import { createRoot } from 'react-dom/client';
import { ExplorerSearch } from '/src/features/files/ExplorerSearch.tsx';
import { Explorer } from '/src/features/files/Explorer.tsx';
import { AowPanelStack } from '/src/components/AowPanel.tsx';
import MonacoEditor from '/src/features/editor/MonacoEditor.tsx';
function App() {
  const [root, changeRoot] = React.useState('/project'); window.changeRoot = changeRoot;
  const [reveal, setReveal] = React.useState();
  const [value, setValue] = React.useState('😀 中文 needle'); window.changeText = setValue;
  return React.createElement('div', { style: { display: 'flex', width: 1000, height: 650 } },
    React.createElement('div', { style: { width: 340, height: '100%' } },
      React.createElement(ExplorerSearch, { projectRoot: root, notesRoot: '/notes', visible: true, projectRefresh: 0, notesRefresh: 0,
        onOpen: (path, source, match) => { window.opened = { path, source, match }; if (match) setReveal({ line: match.line, column: match.column, length: match.match_length, revision: Date.now() }); } },
        React.createElement(AowPanelStack, null, ...[[root, 'Project Explorer'], ['/notes', 'Notes Explorer']].map(([path, title]) =>
          React.createElement(Explorer, { key: title, root: path, title, aowHeader: true, lockedRoot: true, onRootChange: () => {}, onOpenFile: () => {}, refreshRequest: { generation: 0, directory: path } }))))),
    React.createElement('div', { style: { width: 660 } }, React.createElement(MonacoEditor, { path: '/test-file', language: 'plaintext', value, revealLocation: reveal,
      onMount: editor => { window.editor = editor; } })));
}
createRoot(document.getElementById('root')).render(React.createElement(App));
</script></body></html>`;

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } },
  plugins: [{ name: 'search-fixture', configureServer(server) {
    server.middlewares.use('/search-test', async (_request, response, next) => {
      try { response.setHeader('Content-Type', 'text/html'); response.end(await server.transformIndexHtml('/search-test', harness)); } catch (error) { next(error); }
    });
  } }],
});
const entry = (path, kind = 'file') => ({ path, name: path.split('/').pop(), kind, size: 1, hidden: false, readonly: false });
const match = { line: 1, column: 7, match_length: 6, text: '😀 中文 needle', preview_column: 6, preview_length: 6 };
let browser;

async function fixture(t) {
  const context = await browser.newContext({ viewport: { width: 1100, height: 800 }, reducedMotion: 'reduce' });
    await context.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } });
  const state = { requests: [], cancelled: [], errors: [], gates: [] };
  await context.route('**/api/**', async route => {
    const request = route.request(), url = new URL(request.url());
    if (url.pathname === '/api/git/ignored') return route.fulfill({ json: { ignored: [] } });
    if (url.pathname.startsWith('/api/fs/tree/')) {
      const path = url.pathname.slice('/api/fs/tree'.length);
      return route.fulfill({ json: { path, entries: path === '/project' ? [entry('/project/src', 'directory')]
        : path === '/project/src' ? [entry('/project/src/original.txt')] : [] } });
    }
    if (request.method() === 'DELETE') { state.cancelled.push(url.pathname.split('/').pop()); return route.fulfill({ status: 204 }); }
    if (url.pathname === '/api/fs/search') {
      const input = request.postDataJSON(); state.requests.push(input);
      if (input.query === 'network-failure') return route.abort('failed');
      if (input.query === 'slow') await new Promise(resolve => state.gates.push(resolve));
      if (input.use_regex && input.query === '[') return route.fulfill({ status: 400, json: { message: 'regex parse error: unclosed character class' } });
      const files = input.query === 'none' ? [] : input.query === 'many'
        ? Array.from({ length: 1000 }, (_, index) => ({ path: `${input.root}/src/file-${String(index).padStart(4, '0')}.txt`, relative_path: `src/file-${String(index).padStart(4, '0')}.txt`, matches: [] }))
        : [{ path: `${input.root}/src/deep/search.txt`, relative_path: 'src/deep/search.txt', matches: input.mode === 'content' ? [{ ...match, column: input.query === 'long-line' ? 402 : match.column }] : [] }];
      return route.fulfill({ json: { files, total_matches: files.length, truncated: input.query === 'limited' } }).catch(() => undefined);
    }
    return route.fulfill({ status: 404 });
  });
  const page = await context.newPage();
    await page.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } }); page.setDefaultTimeout(10000); page.setDefaultNavigationTimeout(30000);
  page.on('pageerror', error => state.errors.push(error.message));
  t.after(async () => { state.gates.forEach(resolve => resolve()); await context.close(); assert.deepEqual(state.errors, []); });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/search-test`);
  await page.locator('[data-tree-path="/project/src"]').waitFor();
  return { page, state, input: page.getByRole('textbox', { name: '搜索 Project 和 Notes' }) };
}

try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('shared name query searches both roots, preserves hierarchy and restores ordinary tree expansion', async t => {
    const { page, state, input } = await fixture(t);
    await page.locator('[data-tree-path="/project/src"]').click();
    await page.locator('[data-tree-path="/project/src/original.txt"]').waitFor();
    await input.fill('search');
    const results = page.getByRole('tree', { name: '项目资源管理器 搜索结果' });
    await results.getByText('search.txt', { exact: true }).waitFor();
    assert.deepEqual(new Set(state.requests.map(request => request.root)), new Set(['/project', '/notes']));
    assert.equal(await results.getByRole('treeitem', { name: 'deep 1' }).getAttribute('aria-level'), '3');
    await page.getByRole('tree', { name: '笔记资源管理器 搜索结果' }).getByText('search.txt', { exact: true }).click();
    assert.equal((await page.evaluate(() => window.opened)).source, 'notes');
    await page.getByRole('button', { name: '清空搜索' }).click();
    await page.locator('[data-tree-path="/project/src/original.txt"]').waitFor({ state: 'visible' });
  });
  await test('content options rerun both roots and match clicks reveal the exact Unicode selection', async t => {
    const { page, state, input } = await fixture(t);
    await input.fill('needle');
    await page.getByRole('button', { name: '内容', exact: true }).click();
    await page.getByRole('button', { name: '区分大小写' }).click();
    await page.getByRole('button', { name: '全词匹配' }).click();
    await page.getByRole('button', { name: '正则表达式' }).click();
    const results = page.getByRole('tree', { name: '项目资源管理器 搜索结果' });
    await results.locator('mark').waitFor();
    assert.equal(await results.locator('mark').textContent(), 'needle');
    if (process.env.AOW_EXPLORER_SCREENSHOT) await page.screenshot({ path: process.env.AOW_EXPLORER_SCREENSHOT });
    const latest = state.requests.at(-1);
    assert.equal(latest.mode, 'content'); assert.ok(latest.case_sensitive && latest.whole_word && latest.use_regex);
    await page.waitForFunction(() => Boolean(window.editor));
    await results.locator('mark').click();
    await page.waitForFunction(() => window.editor.getSelection()?.startColumn === 7);
    assert.deepEqual(await page.evaluate(() => { const selection = window.editor.getSelection(); return [selection.startLineNumber, selection.startColumn, selection.endColumn]; }), [1, 7, 13]);
    await page.getByRole('button', { name: '搜索范围' }).click();
    await page.getByLabel('包含文件', { exact: true }).fill('*.{ts,txt}');
    await page.getByLabel('包含被忽略的文件').check();
    await page.waitForFunction(() => document.querySelector('[role=tree] mark'));
    await results.locator('mark').waitFor();
    assert.equal(state.requests.at(-1).include_pattern, '*.{ts,txt}');
    assert.equal(state.requests.at(-1).include_ignored, true);
    await page.evaluate(() => window.changeText(`${'x'.repeat(400)} needle`));
    await input.fill('long-line');
    await results.locator('mark').click();
    await page.waitForFunction(() => window.editor.getSelection()?.startColumn === 402);
    assert.ok(await page.evaluate(() => {
      const position = window.editor.getScrolledVisiblePosition({ lineNumber: 1, column: 402 });
      return position && position.left >= 0 && position.left < window.editor.getLayoutInfo().width;
    }), 'long-line match must be visible horizontally');
  });
  await test('cancelled and stale searches never replace the latest scope; errors and truncation stay visible', async t => {
    const { page, state, input } = await fixture(t);
    await input.fill('slow');
    await page.waitForTimeout(450);
    await input.fill('latest');
    await page.getByRole('tree', { name: '项目资源管理器 搜索结果' }).waitFor();
    assert.ok(state.cancelled.length >= 2);
    state.gates.forEach(resolve => resolve()); state.gates.length = 0;
    await page.evaluate(() => window.changeRoot('/other'));
    await page.getByRole('tree', { name: '项目资源管理器 搜索结果' }).getByText('other', { exact: true }).waitFor();
    await input.fill('none');
    await page.getByText('无匹配文件', { exact: true }).first().waitFor();
    await input.fill('network-failure');
    await page.getByRole('alert').first().waitFor();
    const failedIds = state.requests.filter(request => request.query === 'network-failure').map(request => request.request_id);
    await page.waitForTimeout(100);
    assert.equal(failedIds.length, 2);
    assert.ok(failedIds.every(id => state.cancelled.includes(id)));
    await page.getByRole('button', { name: '内容', exact: true }).click();
    await page.getByRole('button', { name: '正则表达式' }).click();
    await input.fill('[');
    await page.getByRole('alert').first().waitFor();
    assert.match(await page.getByRole('alert').first().textContent(), /regex parse error/);
    await input.fill('limited');
    await page.getByText(/结果不完整/).first().waitFor();
    await input.press('Escape');
    assert.equal(await input.inputValue(), '');
  });
  await test('large filtered trees virtualize rows and keyboard navigation keeps the query fixed', async t => {
    const { page, input } = await fixture(t);
    await input.fill('many');
    const results = page.getByRole('tree', { name: '项目资源管理器 搜索结果' });
    await results.getByText('file-0000.txt', { exact: true }).waitFor();
    assert.ok(await page.getByRole('treeitem').count() < 80);
    const top = (await input.boundingBox()).y;
    await results.focus();
    await page.keyboard.press('End');
    await results.getByText('file-0999.txt', { exact: true }).waitFor();
    await page.keyboard.press('Enter');
    assert.equal((await page.evaluate(() => window.opened)).path, '/project/src/file-0999.txt');
    assert.equal((await input.boundingBox()).y, top);
  });
} finally { await browser?.close(); await server.close(); }
