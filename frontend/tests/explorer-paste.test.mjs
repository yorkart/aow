import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const harness = `<!doctype html><html><head><link rel="stylesheet" href="/src/styles.css"></head><body>
<div id="root"></div><input aria-label="Outside editor"><script type="module">
import React from 'react';
import { createRoot } from 'react-dom/client';
import { Explorer } from '/src/features/files/Explorer.tsx';
import { AowPanelStack } from '/src/components/AowPanel.tsx';
function App() {
  const [root, setRoot] = React.useState('/project'); window.changeRoot = setRoot;
  const panelMode = new URLSearchParams(location.search).has('panels');
  if (panelMode) return React.createElement('div', { style: { height: 650, width: 430, marginLeft: 500 } },
    React.createElement(AowPanelStack, null, ...[[root, 'Project Explorer'], ['/notes', 'Notes Explorer']].map(([root, title]) =>
      React.createElement(Explorer, { key: title, root, title, aowHeader: true, lockedRoot: true,
        onRootChange: () => {}, onOpenFile: () => {}, refreshRequest: { generation: 0, directory: root } }))));
  return React.createElement('div', { style: { display: 'flex', height: 650, width: 900 } },
    ...[[root, 'Project Explorer'], ['/notes', 'Notes Explorer']].map(([root, title]) => React.createElement('div', { key: title, style: { display: 'flex', flexDirection: 'column', width: 430 } },
      React.createElement(Explorer, { root, title, lockedRoot: true, refreshRequest: { generation: 0, directory: root },
        onRootChange: () => {}, onOpenFile: () => {}, onRenameFile: async (path, name) => ({ path, name }),
        onCreateFile: async () => {}, onCreateDirectory: async () => {},
      }))));
}
createRoot(document.getElementById('root')).render(React.createElement(App));
</script></body></html>`;
const server = await createServer({
  root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } },
  plugins: [{ name: 'explorer-fixture', configureServer(server) {
    server.middlewares.use('/explorer-test', async (_request, response, next) => {
      try {
        response.setHeader('Content-Type', 'text/html'); response.end(await server.transformIndexHtml('/explorer-test', harness));
      } catch (error) { next(error); }
    });
  } }],
});
const entry = (path, kind = 'file') => ({ path, name: path.split('/').pop(), kind, size: 1, hidden: false, readonly: false });
const tree = (page, title = 'Project Explorer') => page.getByLabel(`${title} 文件树`, { exact: true });
const explorer = (page, title = 'Project Explorer') => tree(page, title).locator('..').locator('..');
const row = (page, path) => page.locator(`.tree-row[data-tree-path="${path}"]`);
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };
let browser;

async function fixture(t, { panelMode = false } = {}) {
  const context = await browser.newContext({ viewport: { width: 1100, height: 800 }, reducedMotion: 'reduce', permissions: ['clipboard-read', 'clipboard-write'] });
  const state = { writes: [], errors: [], gate: null, fail: false, refreshFails: false };
  const directories = new Map([
    ['/project', [entry('/project/assets', 'directory'), entry('/project/other', 'directory'), entry('/project/readme.txt')]],
    ['/project/assets', [entry('/project/assets/existing.txt')]], ['/project/other', []],
    ['/notes', []], ['/new-root', []],
  ]);
  if (panelMode) directories.get('/project').push(...Array.from({ length: 2000 }, (_, index) => entry(`/project/file-${String(index).padStart(4, '0')}.txt`)));
  await context.route('**/api/**', async route => {
    const request = route.request(); const url = new URL(request.url());
    if (url.pathname === '/api/git/ignored') { await route.fulfill({ json: { ignored: [] } }); return; }
    if (url.pathname.startsWith('/api/fs/tree/')) {
      const path = decodeURIComponent(url.pathname.slice('/api/fs/tree'.length));
      await route.fulfill(state.refreshFails ? { status: 500, json: { message: 'Refresh failed' } } : { json: { path, entries: directories.get(path) ?? [] } }); return;
    }
    if (url.pathname.startsWith('/api/fs/file/')) {
      const path = decodeURIComponent(url.pathname.slice('/api/fs/file'.length));
      state.writes.push({ path, body: request.postDataBuffer(), keepBoth: url.searchParams.get('keep_both') });
      if (state.gate) await state.gate.promise;
      if (state.fail && path.endsWith('/fail.txt')) { await route.fulfill({ status: 403, json: { message: 'Permission denied' } }); return; }
      const directory = path.slice(0, path.lastIndexOf('/')); const entries = directories.get(directory);
      let destination = path;
      if (entries.some(item => item.path === path)) destination = path.replace(/(\.[^/.]+)?$/, ' (1)$1');
      entries.push(entry(destination));
      await route.fulfill({ status: 201, json: { path: destination, size: request.postDataBuffer()?.length ?? 0, created: true, version: 'v1' } }); return;
    }
    await route.fulfill({ status: 404, json: { message: url.pathname } });
  });
  const page = await context.newPage(); page.setDefaultTimeout(8000);
  page.setDefaultNavigationTimeout(30000);
  page.on('pageerror', error => state.errors.push(error.stack ?? error.message));
  t.after(async () => { state.gate?.resolve(); await context.close(); assert.deepEqual(state.errors, []); });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/explorer-test${panelMode ? '?panels' : ''}`);
  await row(page, '/project/assets').waitFor();
  return { page, state };
}

async function pasteFiles(target, files) {
  return target.evaluate((element, files) => {
    const data = new DataTransfer();
    for (const file of files) data.items.add(new File([file.content ?? 'contents'], file.name, { type: file.type ?? 'text/plain' }));
    const event = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data });
    element.dispatchEvent(event); return event.defaultPrevented;
  }, files);
}

async function useImageClipboard(page) {
  await page.evaluate(async () => {
    const canvas = document.createElement('canvas'); canvas.width = 2; canvas.height = 2;
    canvas.getContext('2d').fillRect(0, 0, 2, 2);
    const blob = await new Promise(resolve => canvas.toBlob(resolve, 'image/png'));
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
  });
}

try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  await test('context-menu image paste uses the real browser clipboard and reveals the uploaded screenshot', async t => {
    const { page, state } = await fixture(t);
    await useImageClipboard(page);
    await row(page, '/project/assets').click({ button: 'right' });
    await page.getByRole('menuitem', { name: /粘贴/ }).click();
    await explorer(page).getByText('已上传', { exact: true }).waitFor();
    assert.equal(state.writes.length, 1);
    assert.match(state.writes[0].path, /^\/project\/assets\/screenshot-.*\.png$/);
    assert.equal(state.writes[0].keepBoth, 'true');
    assert.equal(state.writes[0].body.subarray(1, 4).toString(), 'PNG');
    await row(page, state.writes[0].path).waitFor();
    assert.equal(await row(page, state.writes[0].path).getAttribute('class').then(value => value.includes('uploaded')), true);
  });

  await test('native keyboard image paste is scoped to the focused Notes tree', async t => {
    const { page, state } = await fixture(t);
    await useImageClipboard(page);
    await tree(page, 'Notes Explorer').focus();
    await page.keyboard.press('Control+V');
    await explorer(page, 'Notes Explorer').getByText('已上传', { exact: true }).waitFor();
    assert.equal(state.writes.length, 1); assert.match(state.writes[0].path, /^\/notes\/screenshot-/);
    await page.getByLabel('Outside editor').focus();
    assert.equal(await pasteFiles(page.getByLabel('Outside editor'), [{ name: 'outside.txt' }]), false);
    assert.equal(state.writes.length, 1);
  });

  await test('file targets use their parent, queues retain destinations, and per-file failures can retry', async t => {
    const { page, state } = await fixture(t);
    await row(page, '/project/assets').click();
    await row(page, '/project/assets/existing.txt').click();
    state.gate = deferred(); state.fail = true;
    assert.equal(await pasteFiles(tree(page), [{ name: 'existing.txt', content: 'new' }, { name: 'fail.txt' }, { name: 'empty.txt', content: '' }]), true);
    await explorer(page).getByLabel('上传进度 existing.txt').waitFor();
    await row(page, '/project/other').click();
    state.gate.resolve();
    await explorer(page).getByText('Permission denied', { exact: true }).waitFor();
    await row(page, '/project/assets/empty.txt').waitFor();
    assert.deepEqual(state.writes.map(item => item.path), ['/project/assets/existing.txt', '/project/assets/fail.txt', '/project/assets/empty.txt']);
    await row(page, '/project/assets/existing (1).txt').waitFor();
    state.fail = false;
    await explorer(page).getByRole('button', { name: '重试', exact: true }).click();
    await row(page, '/project/assets/fail.txt').waitFor();
    assert.equal(state.writes.at(-1).path, '/project/assets/fail.txt');
  });

  await test('denied menu reads keep the target for keyboard paste or file selection; blank space uses root', async t => {
    const { page, state } = await fixture(t);
    await page.evaluate(() => { navigator.clipboard.read = async () => { throw new DOMException('Denied', 'NotAllowedError'); }; });
    await row(page, '/project/assets').click({ button: 'right' });
    await page.getByRole('menuitem', { name: /粘贴/ }).click();
    await explorer(page).getByText(/无法读取剪贴板/).waitFor();
    assert.equal(await tree(page).evaluate(element => element === document.activeElement), true);
    await pasteFiles(tree(page), [{ name: 'fallback.pdf', type: 'application/pdf' }]);
    await row(page, '/project/assets/fallback.pdf').waitFor();
    await tree(page).click({ button: 'right', position: { x: 350, y: 380 } });
    await page.getByRole('menuitem', { name: /粘贴/ }).click();
    await explorer(page).getByText('上传到：/project', { exact: true }).waitFor();
    await explorer(page).getByLabel('选择粘贴上传文件').setInputFiles({ name: 'selected.bin', mimeType: 'application/octet-stream', buffer: Buffer.from([0, 255]) });
    await row(page, '/project/selected.bin').waitFor();
    assert.equal(state.writes.at(-1).path, '/project/selected.bin');
  });

  await test('plain text, rename inputs and multi-format clipboard data are handled without duplicate uploads', async t => {
    const { page, state } = await fixture(t);
    const prevented = await tree(page).evaluate(element => {
      const data = new DataTransfer(); data.setData('text/plain', '/local/not-a-file.txt');
      const event = new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: data });
      element.dispatchEvent(event); return event.defaultPrevented;
    });
    assert.equal(prevented, false); assert.equal(state.writes.length, 0);
    await row(page, '/project/readme.txt').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '重命名', exact: true }).click();
    const rename = page.getByLabel('新文件名', { exact: true });
    await rename.waitFor();
    assert.equal(await pasteFiles(rename, [{ name: 'ignored.txt' }]), false);
    await rename.press('Escape');
    await page.evaluate(() => {
      navigator.clipboard.read = async () => [{ types: ['text/html', 'image/png', 'image/jpeg'], getType: async type => new Blob(['image'], { type }) }];
    });
    await row(page, '/project').click({ button: 'right' });
    await page.getByRole('menuitem', { name: /粘贴/ }).click();
    await explorer(page).getByText('已上传', { exact: true }).waitFor();
    assert.equal(state.writes.length, 1);
  });

  await test('late clipboard reads cannot upload into a new root, and refresh failure does not invite re-upload', async t => {
    const { page, state } = await fixture(t);
    await page.evaluate(() => { navigator.clipboard.read = () => new Promise(resolve => { window.finishRead = () => resolve([{ types: ['image/png'], getType: async () => new Blob(['image'], { type: 'image/png' }) }]); }); });
    await row(page, '/project').click({ button: 'right' });
    await page.getByRole('menuitem', { name: /粘贴/ }).click();
    await page.evaluate(() => window.changeRoot('/new-root'));
    await row(page, '/new-root').waitFor();
    await page.evaluate(() => window.finishRead());
    assert.equal(state.writes.length, 0);
    state.refreshFails = true;
    await pasteFiles(tree(page), [{ name: 'saved.txt' }]);
    await explorer(page).getByText(/文件已上传，目录刷新失败/).waitFor();
    assert.equal(await explorer(page).getByRole('button', { name: '重试', exact: true }).count(), 0);
    assert.equal(state.writes.at(-1).path, '/new-root/saved.txt');
  });

  await test('upload refresh preserves an active rename draft and cancel stops queued files', async t => {
    const { page, state } = await fixture(t);
    state.gate = deferred();
    await pasteFiles(tree(page), [{ name: 'new.txt' }]);
    await explorer(page).getByLabel('上传进度 new.txt').waitFor();
    await row(page, '/project/readme.txt').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '重命名', exact: true }).click();
    const rename = page.getByLabel('新文件名', { exact: true });
    await rename.fill('draft.txt');
    state.gate.resolve();
    await row(page, '/project/new.txt').waitFor();
    assert.equal(await rename.inputValue(), 'draft.txt');
    assert.equal(await rename.evaluate(element => element === document.activeElement), true);
    await rename.press('Escape');
    state.gate = deferred();
    await pasteFiles(tree(page), [{ name: 'cancel.txt' }, { name: 'queued.txt' }]);
    await explorer(page).getByLabel('上传进度 cancel.txt').waitFor();
    await explorer(page).getByRole('button', { name: '取消上传', exact: true }).click();
    await explorer(page).getByText('上传已取消', { exact: true }).first().waitFor();
    assert.equal(state.writes.some(item => item.path.endsWith('/queued.txt')), false);
  });
  await test('panel explorers share one virtual viewport, preserve expanded folders and reveal pasted files', async t => {
    const { page } = await fixture(t, { panelMode: true });
    const scroller = page.locator('.aow-panel-viewport');
    assert.equal(await page.getByRole('button', { name: '展开 Notes Explorer', exact: true }).count(), 1);
    assert.ok(await page.locator('.tree-row').count() < 100);
    await row(page, '/project/assets').click();
    await row(page, '/project/assets/existing.txt').waitFor();
    await page.getByRole('button', { name: '收起 Project Explorer', exact: true }).click();
    await page.getByRole('button', { name: '展开 Project Explorer', exact: true }).click();
    await row(page, '/project/assets/existing.txt').waitFor();
    await scroller.evaluate(element => { element.scrollTop = element.scrollHeight; });
    await row(page, '/project/file-1999.txt').waitFor();
    assert.ok(await page.locator('.tree-row').count() < 100);
    await page.getByRole('button', { name: '定位 Notes Explorer', exact: true }).click();
    await page.getByRole('button', { name: '收起 Notes Explorer', exact: true }).waitFor();
    await pasteFiles(tree(page, 'Notes Explorer'), [{ name: 'note.txt' }]);
    await row(page, '/notes/note.txt').waitFor();
    await page.getByRole('button', { name: '定位 Project Explorer', exact: true }).click();
    await row(page, '/project/assets').waitFor();
    await tree(page).dispatchEvent('click');
    await pasteFiles(tree(page), [{ name: 'zz-uploaded.txt' }]);
    await row(page, '/project/zz-uploaded.txt').waitFor();
    assert.ok(await scroller.evaluate(element => element.scrollTop > 1000));
    assert.ok(await page.locator('.tree-row').count() < 100);
    assert.equal(await page.locator('.tree-list').evaluateAll(elements => elements.some(element => /auto|scroll/.test(getComputedStyle(element).overflowY))), false);
    await row(page, '/project/zz-uploaded.txt').click({ button: 'right' });
    const menu = page.getByRole('menu', { name: '/project/zz-uploaded.txt 操作', exact: true });
    await menu.waitFor();
    const menuBounds = await menu.boundingBox();
    assert.ok(menuBounds.x >= 0 && menuBounds.x + menuBounds.width <= 1100);
    const copy = menu.getByRole('menuitem', { name: '复制 • 文件名', exact: true });
    assert.equal(await copy.evaluate(element => { const box = element.getBoundingClientRect(); return element.contains(document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)); }), true);
    await copy.click();
    await menu.waitFor({ state: 'hidden' });
  });
} finally { await browser?.close(); await server.close(); }
