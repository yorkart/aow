import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, readFile } from 'node:fs/promises';
import { chromium } from 'playwright';
import { productionPreview } from './fixtures/production-preview.mjs';

// Browser downloads can bypass Playwright routing; serve the raw response over HTTP.
const server = await productionPreview({
  plugins: [{ name: 'file-download-fixture', configurePreviewServer(server) {
    server.middlewares.use((request, response, next) => {
      if (!request.url.startsWith('/api/fs/raw/')) { next(); return; }
      response.setHeader('Content-Type', 'text/plain');
      response.end('downloaded file contents');
    });
  } }],
});
const baseURL = `http://127.0.0.1:${server.httpServer.address().port}`;
const home = '/home/test';
const specialName = '报告 #1 &+?.txt';
const longDirectoryName = 'deeply-nested-directory-with-a-long-name-that-needs-horizontal-scrolling-完整目录名称';
const deepSegments = [...Array.from({ length: 22 }, (_, index) => `level-${index}`), longDirectoryName];
const deepDirectoryPath = `${home}/Documents/${deepSegments.join('/')}`;
const file = (directory, name, kind = 'file') => ({ name, path: `${directory === '/' ? '' : directory}/${name}`, kind, size: 2048, modified_ms: 1_789_344_000_000, readonly: false, hidden: name.startsWith('.'), mode: 33188, links: 1, uid: 1000, gid: 1000, is_symlink: false, link_target: null });
const deferred = () => { let resolve; const promise = new Promise((done) => { resolve = done; }); return { promise, resolve }; };

async function fixture(browser, options = {}) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
  const state = { reads: [], writes: [], errors: [], uploadGate: null, slowGate: null, failHome: options.failHome ?? false, deniedPaths: new Set(),
    pins: options.pins ?? { paths: [], revision: 0 }, pinMutations: [], failPinReads: false, failPinWrites: false, pinGate: null, delayedPin: '' };
  const directories = new Map([
    ['/', [file('/', 'home', 'directory'), file('/', 'tmp', 'directory'), ...Array.from({ length: 45 }, (_, index) => file('/', `folder-${index}`, 'directory'))]],
    ['/home', [file('/home', 'test', 'directory')]],
    [home, [file(home, 'Documents', 'directory'), file(home, '.config', 'directory'), file(home, specialName), ...Array.from({ length: 70 }, (_, index) => file(home, `file-${String(index).padStart(2, '0')}.txt`))]],
    [`${home}/Documents`, [file(`${home}/Documents`, 'nested', 'directory')]],
    [`${home}/Documents/nested`, [file(`${home}/Documents/nested`, longDirectoryName, 'directory')]],
    [`${home}/.config`, []],
    ['/tmp', []],
    ['/slow', []],
  ]);
  if (options.deepTree) {
    let parent = `${home}/Documents`;
    for (const name of deepSegments) {
      directories.get(parent).push(file(parent, name, 'directory'));
      parent = `${parent}/${name}`;
      directories.set(parent, []);
    }
  }
  await context.route('**/api/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === '/api/aow/pinned-directories') {
      if (request.method() === 'GET' && state.failPinReads) { await route.fulfill({ status: 500, json: { message: 'Pins unavailable' } }); return; }
      let payload;
      if (request.method() === 'PATCH') {
        const update = request.postDataJSON();
        state.pinMutations.push(update);
        if (state.failPinWrites) { await route.fulfill({ status: 500, json: { message: 'Config write failed' } }); return; }
        state.pins.paths = [...new Set([...state.pins.paths.filter((path) => !update.remove?.includes(path)), ...update.add ?? []])];
        state.pins.revision += 1;
        payload = { paths: [...state.pins.paths], revision: state.pins.revision };
        if (update.add?.includes(state.delayedPin)) await state.pinGate?.promise;
      }
      await route.fulfill({ json: payload ?? state.pins });
      return;
    }
    if (url.pathname.startsWith('/api/fs/file') && request.method() === 'PUT') {
      const path = decodeURIComponent(url.pathname.slice('/api/fs/file'.length));
      state.writes.push({ path, body: request.postDataBuffer() });
      if (state.uploadGate) await state.uploadGate.promise;
      if (path.endsWith('/fail.txt')) { await route.fulfill({ status: 403, json: { message: 'Permission denied' } }); return; }
      const parent = path.slice(0, path.lastIndexOf('/')) || '/';
      directories.get(parent).push(file(parent, path.split('/').pop()));
      await route.fulfill({ json: { path, size: request.postDataBuffer()?.length ?? 0, created: true, version: 'v1' } });
      return;
    }
    if (url.pathname.startsWith('/api/fs/raw')) {
      await route.continue();
      return;
    }
    if (url.pathname.startsWith('/api/fs/')) {
      state.reads.push(url.pathname);
      if (url.pathname === '/api/fs/home' && state.failHome) { await route.fulfill({ status: 403, json: { message: 'Home unavailable' } }); return; }
      const path = url.pathname === '/api/fs/home' ? home : decodeURIComponent(url.pathname.slice('/api/fs/tree'.length)) || '/';
      if (path === '/slow' && state.slowGate) await state.slowGate.promise;
      if (!directories.has(path) || state.deniedPaths.has(path)) { await route.fulfill({ status: 403, json: { message: 'Permission denied' } }); return; }
      await route.fulfill({ json: { path, entries: directories.get(path) } });
      return;
    }
    const data = {
      '/api/auth/status': { configured: true, authenticated: true },
      '/api/aow/projects': [{ id: '__aow_floating', name: '浮动工作区', builtin: true, registered_path: '/global', notes_path: '/notes/global', common_git_dir: '/global/.git', worktrees: [{ id: 'global-main', project_id: '__aow_floating', path: '/global', branch: 'main', head: '', is_main: true, detached: false, locked: false, prunable: false, color: 'default' }] }],
      '/api/terminals': [],
      '/api/aow/agents': [],
      '/api/aow/pinned-worktrees': { paths: [], revision: 0 },
      '/api/aow/settings': { notes_base: '/notes' },
    }[url.pathname];
    await route.fulfill(data ? { json: data } : { status: 404, json: { message: `Unexpected API: ${url.pathname}` } });
  });
  const page = await context.newPage();
  page.on('pageerror', (error) => state.errors.push(error.message));
  await page.goto(`${baseURL}/?ui=desktop`);
  await page.getByRole('button', { name: '浮动工作区', exact: true }).waitFor();
  return { context, page, state };
}

async function openBrowser(page) {
  const trigger = page.getByRole('button', { name: '浮动工作区', exact: true });
  await trigger.waitFor();
  if (await trigger.getAttribute('aria-expanded') !== 'true') await trigger.click();
  await page.getByRole('dialog', { name: '浮动工作区' }).waitFor();
  if (!await page.locator('.system-file-browser').isVisible()) {
    await page.getByRole('button', { name: '打开系统文件浏览器', exact: true }).click();
    await page.locator('.system-file-browser').waitFor();
  }
}

async function atPath(page, path) {
  await page.waitForFunction((value) => document.querySelector('.system-file-address input')?.value === value && document.querySelector('.system-file-list')?.getAttribute('aria-busy') === 'false', path);
}

let browser;
try {
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  await test('home, shortcuts, tree navigation, clipboard, downloads and hidden state', async () => {
    const { context, page, state } = await fixture(browser);
    try {
      assert.equal(await page.getByRole('button', { name: 'Settings', exact: true }).textContent(), '');
      assert.equal(state.reads.length, 0, 'filesystem should load on first use');
      await openBrowser(page);
      await atPath(page, home);
      assert.equal(await page.getByRole('button', { name: 'Home', exact: true }).getAttribute('aria-pressed'), 'true');
      await page.getByRole('button', { name: `展开 ${home}/Documents`, exact: true }).click();
      await page.locator('.system-file-tree').getByRole('button', { name: 'nested', exact: true }).waitFor();
      await page.getByRole('button', { name: `展开 ${home}/Documents/nested`, exact: true }).click();
      const longName = page.locator('.system-file-tree').getByRole('button', { name: longDirectoryName, exact: true }).locator('span');
      await longName.waitFor();
      assert.equal(await longName.evaluate((node) => node.clientWidth >= node.scrollWidth), true, 'long directory names should retain their full width');
      await page.getByRole('button', { name: `复制路径 ${specialName}`, exact: true }).click();
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${home}/${specialName}`);
      await page.getByRole('button', { name: '复制路径 Documents', exact: true }).click();
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${home}/Documents`);
      const downloadEvent = page.waitForEvent('download');
      await page.getByRole('link', { name: `下载 ${specialName}`, exact: true }).click();
      const download = await downloadEvent;
      // Chromium replaces characters that cannot be used in filenames on every platform.
      assert.equal(download.suggestedFilename(), specialName.replace('?', '_'));
      assert.equal(await readFile(await download.path(), 'utf8'), 'downloaded file contents');

      await page.getByRole('button', { name: 'file-30.txt', exact: true }).click();
      const scroll = await page.evaluate(() => {
        const tree = document.querySelector('.system-file-tree');
        const list = document.querySelector('.system-file-list');
        tree.scrollTop = 180;
        tree.scrollLeft = 240;
        list.scrollTop = 700;
        return [tree.scrollTop, tree.scrollLeft, list.scrollTop];
      });
      assert.ok(scroll.every((position) => position > 0));
      assert.equal(await page.evaluate(() => {
        const body = document.querySelector('.system-file-body');
        const tree = document.querySelector('.system-file-tree');
        return tree.scrollWidth > tree.clientWidth && body.scrollWidth === body.clientWidth && document.documentElement.scrollWidth === window.innerWidth;
      }), true, 'horizontal overflow must stay inside the directory tree');
      const reads = state.reads.length;
      await page.getByRole('button', { name: '最小化浮动工作区' }).click();
      await page.getByRole('dialog').waitFor({ state: 'hidden' });
      assert.equal(await page.locator('.system-file-browser').count(), 1, 'hidden dialog remains mounted');
      await openBrowser(page);
      await atPath(page, home);
      assert.equal(state.reads.length, reads, 'reopening must not reload directories');
      assert.deepEqual(await page.evaluate(() => [document.querySelector('.system-file-tree').scrollTop, document.querySelector('.system-file-tree').scrollLeft, document.querySelector('.system-file-list').scrollTop]), scroll);
      assert.equal(await page.locator('.system-file-list tr.selected .system-file-entry-name').textContent(), 'file-30.txt');
      assert.equal(await page.getByRole('button', { name: `折叠 ${home}/Documents`, exact: true }).getAttribute('aria-expanded'), 'true');
      if (process.env.FILE_TEST_SCREENSHOTS) {
        await mkdir(process.env.FILE_TEST_SCREENSHOTS, { recursive: true });
        await page.screenshot({ path: `${process.env.FILE_TEST_SCREENSHOTS}/system-file-browser.png` });
      }

      await page.getByRole('button', { name: '/tmp', exact: true }).click();
      await atPath(page, '/tmp');
      await page.getByText('目录为空，可拖入文件上传', { exact: true }).waitFor();
      await page.getByRole('button', { name: '/', exact: true }).first().click();
      await atPath(page, '/');
      assert.equal(await page.locator('.system-file-tree').evaluate((node) => node.scrollLeft), 0, 'navigating to the root should reveal its name again');
      assert.equal(await page.getByRole('button', { name: '上级目录', exact: true }).isDisabled(), true);
      assert.ok(state.reads.includes('/api/fs/tree'));
      assert.ok(!state.reads.includes('/api/fs/tree/'));
      await page.getByRole('button', { name: 'Home', exact: true }).click();
      await atPath(page, home);
      await page.locator('.system-file-tree').getByRole('button', { name: 'Documents', exact: true }).click();
      await atPath(page, `${home}/Documents`);
      await page.getByRole('button', { name: '复制当前目录路径' }).click();
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), `${home}/Documents`);
      await page.keyboard.press('Escape');
      await page.getByRole('dialog').waitFor({ state: 'hidden' });
      assert.equal(await page.getByRole('button', { name: '浮动工作区', exact: true }).evaluate((node) => node === document.activeElement), true);
      await openBrowser(page);
      await atPath(page, `${home}/Documents`);
      assert.deepEqual(state.errors, []);
    } finally { await context.close(); }
  });

  await test('deep tree actions stay at the viewport edge and work without horizontal scrolling', async () => {
    const { context, page, state } = await fixture(browser, { deepTree: true, pins: { paths: [deepDirectoryPath], revision: 1 } });
    try {
      await openBrowser(page);
      await atPath(page, home);
      await page.getByRole('button', { name: `打开收藏 ${deepDirectoryPath}`, exact: true }).click();
      await atPath(page, deepDirectoryPath);
      const tree = page.locator('.system-file-tree');
      const name = tree.getByRole('button', { name: longDirectoryName, exact: true });
      await name.waitFor();
      const row = name.locator('..');
      const actions = row.locator('.system-file-tree-actions');
      const maxScroll = await tree.evaluate((node) => node.scrollWidth - node.clientWidth);
      assert.ok(maxScroll > 400, 'the fixture must overflow substantially');
      const bounds = await tree.boundingBox();
      let actionRight;
      for (const target of [0, Math.floor(maxScroll / 2), maxScroll]) {
        const scrollLeft = await tree.evaluate((node, value) => { node.scrollLeft = value; return node.scrollLeft; }, target);
        const actionBounds = await actions.boundingBox();
        assert.ok(actionBounds.x >= bounds.x && actionBounds.x + actionBounds.width <= bounds.x + bounds.width);
        const right = actionBounds.x + actionBounds.width;
        if (actionRight !== undefined) assert.ok(Math.abs(right - actionRight) <= 1, 'actions must remain at the same viewport position');
        actionRight = right;
        assert.equal(await actions.evaluate((node) => getComputedStyle(node).backgroundColor === getComputedStyle(node.parentElement).backgroundColor), true);
        const copy = row.getByRole('button', { name: `复制路径 ${deepDirectoryPath}`, exact: true });
        const copyBounds = await copy.boundingBox();
        // Use pointer coordinates so Playwright cannot scroll the button into view for us.
        await page.mouse.click(copyBounds.x + copyBounds.width / 2, copyBounds.y + copyBounds.height / 2);
        assert.equal(await page.evaluate(() => navigator.clipboard.readText()), deepDirectoryPath);
        assert.equal(await tree.evaluate((node) => node.scrollLeft), scrollLeft);
        const pin = row.locator('.system-file-tree-pin');
        const wasPinned = await pin.getAttribute('aria-pressed') === 'true';
        const pinBounds = await pin.boundingBox();
        await page.mouse.click(pinBounds.x + pinBounds.width / 2, pinBounds.y + pinBounds.height / 2);
        await page.waitForFunction(({ path, expected }) => {
          const pin = [...document.querySelectorAll('.system-file-tree-name')].find((node) => node.title === path)?.parentElement.querySelector('.system-file-tree-pin');
          return pin?.getAttribute('aria-pressed') === String(expected) && !pin.disabled;
        }, { path: deepDirectoryPath, expected: !wasPinned });
        assert.equal(await tree.evaluate((node) => node.scrollLeft), scrollLeft);
      }
      const selectedScroll = await tree.evaluate((node) => [node.scrollLeft, node.scrollTop]);
      await page.getByRole('button', { name: '最小化浮动工作区' }).click();
      await openBrowser(page);
      assert.deepEqual(await tree.evaluate((node) => [node.scrollLeft, node.scrollTop]), selectedScroll);

      await page.getByRole('navigation', { name: '快捷导航', exact: true }).getByRole('button', { name: 'Home', exact: true }).click();
      await atPath(page, home);
      await row.evaluate((node) => {
        const tree = node.closest('.system-file-tree');
        const top = node.getBoundingClientRect().top - tree.getBoundingClientRect().top;
        tree.scrollTop += top - 70;
        tree.scrollLeft = 0;
      });
      const hoverBounds = await actions.boundingBox();
      await page.mouse.move(hoverBounds.x + 5, hoverBounds.y + 10);
      assert.equal(await row.locator('.system-file-tree-copy').evaluate((node) => getComputedStyle(node).opacity), '1', 'hover reveals actions even when indentation is outside the viewport');
      await page.mouse.move(10, 10);
      const beforeFocus = await tree.evaluate((node) => node.scrollLeft);
      await row.locator('.system-file-tree-copy').focus();
      assert.equal(await row.locator('.system-file-tree-copy').evaluate((node) => getComputedStyle(node).opacity), '1');
      await page.keyboard.press('Enter');
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), deepDirectoryPath);
      assert.equal(await tree.evaluate((node) => node.scrollLeft), beforeFocus, 'keyboard actions must not move the horizontal viewport');
      if (process.env.FILE_TEST_SCREENSHOTS) {
        await mkdir(process.env.FILE_TEST_SCREENSHOTS, { recursive: true });
        await page.screenshot({ path: `${process.env.FILE_TEST_SCREENSHOTS}/system-file-tree-actions.png` });
      }
      assert.deepEqual(state.errors, []);
    } finally { await context.close(); }
  });

  await test('uploads capture their destination, survive hiding, accept drops and report per-file failures', async () => {
    const { context, page, state } = await fixture(browser);
    try {
      await openBrowser(page);
      await atPath(page, home);
      state.uploadGate = deferred();
      const chooserEvent = page.waitForEvent('filechooser');
      await page.getByRole('button', { name: '上传文件', exact: true }).click();
      await (await chooserEvent).setFiles([
        { name: 'upload #一.txt', mimeType: 'text/plain', buffer: Buffer.from('button upload') },
        { name: 'upload-two.txt', mimeType: 'text/plain', buffer: Buffer.from('second upload') },
      ]);
      await page.getByLabel('上传进度 upload #一.txt', { exact: true }).waitFor();
      await page.getByRole('button', { name: '/tmp', exact: true }).click();
      await atPath(page, '/tmp');
      await page.getByRole('button', { name: '最小化浮动工作区' }).click();
      state.uploadGate.resolve();
      await page.waitForFunction(() => document.querySelectorAll('.system-file-uploads .system-file-success').length === 2);
      assert.deepEqual(state.writes.map((item) => item.path), [`${home}/upload #一.txt`, `${home}/upload-two.txt`]);
      assert.equal(state.writes[0].body.toString(), 'button upload');
      await openBrowser(page);
      await atPath(page, '/tmp');
      assert.equal(await page.getByText('已上传', { exact: true }).count(), 2);
      const transfer = await page.evaluateHandle(() => {
        const data = new DataTransfer();
        data.items.add(new File(['drop upload'], 'drop &一.txt', { type: 'text/plain' }));
        return data;
      });
      await page.locator('.system-file-browser').dispatchEvent('dragenter', { dataTransfer: transfer });
      await page.getByText('松开以上传文件', { exact: true }).waitFor();
      await page.locator('.system-file-browser').dispatchEvent('drop', { dataTransfer: transfer });
      await transfer.dispose();
      await page.locator('.system-file-list').getByRole('button', { name: 'drop &一.txt', exact: true }).waitFor();
      assert.equal(state.writes.at(-1).path, '/tmp/drop &一.txt');
      assert.equal(state.writes.at(-1).body.toString(), 'drop upload');
      await page.getByLabel('选择上传文件').setInputFiles([
        { name: 'fail.txt', mimeType: 'text/plain', buffer: Buffer.from('failure') },
        { name: 'after-failure.txt', mimeType: 'text/plain', buffer: Buffer.from('success') },
      ]);
      await page.locator('.system-file-uploads').getByText('Permission denied', { exact: true }).waitFor();
      await page.locator('.system-file-list').getByRole('button', { name: 'after-failure.txt', exact: true }).waitFor();
      assert.equal(await page.getByText('已上传', { exact: true }).count(), 4);
      await page.getByRole('button', { name: 'Home', exact: true }).click();
      await atPath(page, home);
      await page.locator('.system-file-list').getByRole('button', { name: 'upload #一.txt', exact: true }).waitFor();
      assert.deepEqual(state.errors, []);
    } finally { state.uploadGate?.resolve(); await context.close(); }
  });

  await test('navigation errors and stale responses preserve the current directory, clipboard has a fallback', async () => {
    const { context, page, state } = await fixture(browser);
    try {
      await openBrowser(page);
      await atPath(page, home);
      await page.getByRole('textbox', { name: '目录路径' }).fill('/denied');
      await page.getByRole('button', { name: '前往', exact: true }).click();
      await page.getByRole('alert').getByText('无法打开 /denied：Permission denied', { exact: true }).waitFor();
      assert.equal(await page.getByRole('button', { name: 'Home', exact: true }).getAttribute('aria-pressed'), 'true');
      state.slowGate = deferred();
      await page.getByRole('textbox', { name: '目录路径' }).fill('/slow');
      const slowRequest = page.waitForRequest('**/api/fs/tree/slow');
      await page.getByRole('button', { name: '前往', exact: true }).click();
      await slowRequest;
      await page.getByRole('button', { name: '/tmp', exact: true }).click();
      await atPath(page, '/tmp');
      const slowResponse = page.waitForResponse('**/api/fs/tree/slow');
      state.slowGate.resolve();
      await slowResponse;
      await atPath(page, '/tmp');
      await page.evaluate(() => {
        Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined });
        document.execCommand = (command) => {
          const node = document.activeElement;
          window.fallbackCopy = command === 'copy' && node?.closest('.system-file-browser') ? node.value : null;
          return !!window.fallbackCopy;
        };
      });
      await page.getByRole('button', { name: '复制当前目录路径' }).click();
      assert.equal(await page.evaluate(() => window.fallbackCopy), '/tmp');
      await page.getByRole('textbox', { name: '目录路径' }).fill('~/.config');
      await page.getByRole('button', { name: '前往', exact: true }).click();
      await atPath(page, `${home}/.config`);
      state.deniedPaths.add(`${home}/.config`);
      await page.getByRole('button', { name: '刷新当前目录', exact: true }).click();
      await page.locator('.system-file-tree-error').getByRole('button', { name: '重试', exact: true }).waitFor();
      state.deniedPaths.clear();
      const retryResponse = page.waitForResponse(`**/api/fs/tree${home}/.config`);
      await page.locator('.system-file-tree-error').getByRole('button', { name: '重试', exact: true }).click();
      await retryResponse;
      await page.locator('.system-file-tree-error').waitFor({ state: 'hidden' });
      await atPath(page, `${home}/.config`);
      assert.deepEqual(state.errors, []);
    } finally { state.slowGate?.resolve(); await context.close(); }
  });

  await test('three columns, pins from every directory entry, persistence and tree reveal', async () => {
    const { context, page, state } = await fixture(browser);
    try {
      await openBrowser(page);
      await atPath(page, home);
      const shortcuts = page.getByRole('complementary', { name: '快捷导航与收藏', exact: true });
      const tree = page.getByRole('complementary', { name: '目录树', exact: true });
      const bookmarks = page.getByRole('region', { name: '收藏目录', exact: true });
      const columns = await page.evaluate(() => ['.system-file-shortcuts', '.system-file-tree', '.system-file-list'].map((selector) => {
        const bounds = document.querySelector(selector).getBoundingClientRect();
        return { left: bounds.left, right: bounds.right, top: bounds.top, width: bounds.width };
      }));
      assert.ok(columns[0].right <= columns[1].left && columns[1].right <= columns[2].left);
      assert.ok(columns.every((column) => column.top === columns[0].top && column.width > 100));
      assert.equal(await shortcuts.getByRole('navigation', { name: '快捷导航' }).getByRole('button').count(), 3);
      await page.getByRole('button', { name: 'Pin 当前目录', exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}`, exact: true }).waitFor();
      await page.locator('.system-file-list').getByRole('button', { name: 'Pin Documents', exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).waitFor();
      await tree.getByRole('button', { name: `Pin ${home}/.config`, exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/.config`, exact: true }).waitFor();
      assert.deepEqual(state.pins.paths, [home, `${home}/Documents`, `${home}/.config`]);
      assert.equal(await page.locator('.system-file-list').getByRole('button', { name: `Pin ${specialName}`, exact: true }).count(), 0);

      await tree.getByRole('button', { name: '折叠 /', exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).click();
      await atPath(page, `${home}/Documents`);
      await tree.locator('[aria-current="location"]').waitFor();
      assert.equal(await tree.locator('[aria-current="location"]').getAttribute('title'), `${home}/Documents`);
      await tree.getByRole('button', { name: '折叠 /', exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).click();
      await tree.getByRole('button', { name: '折叠 /', exact: true }).waitFor();
      await tree.evaluate((node) => { node.scrollTop = node.scrollHeight; });
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).click();
      await page.waitForFunction(() => {
        const tree = document.querySelector('.system-file-tree');
        const row = tree.querySelector('[aria-current="location"]');
        const bounds = tree.getBoundingClientRect();
        const rowBounds = row.getBoundingClientRect();
        return rowBounds.top >= bounds.top && rowBounds.bottom <= bounds.bottom;
      });
      await page.getByRole('button', { name: '最小化浮动工作区' }).click();
      await openBrowser(page);
      await atPath(page, `${home}/Documents`);
      assert.equal(await bookmarks.locator('li').count(), 3);
      if (process.env.FILE_TEST_SCREENSHOTS) {
        await mkdir(process.env.FILE_TEST_SCREENSHOTS, { recursive: true });
        await page.screenshot({ path: `${process.env.FILE_TEST_SCREENSHOTS}/system-file-browser-pins.png` });
      }

      await page.reload();
      await openBrowser(page);
      await atPath(page, `${home}/Documents`);
      await page.getByRole('button', { name: 'Home', exact: true }).click();
      await atPath(page, home);
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).waitFor();
      assert.equal(await page.getByRole('button', { name: '取消 Pin 当前目录', exact: true }).getAttribute('aria-pressed'), 'true');
      await bookmarks.getByRole('button', { name: `取消 Pin ${home}/Documents`, exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).waitFor({ state: 'hidden' });
      await tree.getByRole('button', { name: `取消 Pin ${home}/.config`, exact: true }).click();
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/.config`, exact: true }).waitFor({ state: 'hidden' });
      await page.getByRole('button', { name: '取消 Pin 当前目录', exact: true }).click();
      await bookmarks.getByText('Pin 常用目录，在这里快速访问。', { exact: true }).waitFor();
      await page.reload();
      await openBrowser(page);
      await atPath(page, home);
      await bookmarks.getByText('Pin 常用目录，在这里快速访问。', { exact: true }).waitFor();
      assert.deepEqual(state.pins.paths, []);
      assert.deepEqual(state.errors, []);
    } finally { await context.close(); }
  });

  await test('pin failures preserve saved bookmarks and unavailable directories remain removable', async () => {
    const { context, page, state } = await fixture(browser, { pins: { paths: ['/missing'], revision: 1 } });
    try {
      state.failPinReads = true;
      await openBrowser(page);
      await atPath(page, home);
      const bookmarks = page.getByRole('region', { name: '收藏目录', exact: true });
      await bookmarks.getByRole('alert').waitFor();
      state.failPinReads = false;
      await bookmarks.getByRole('button', { name: '重新加载', exact: true }).click();
      await bookmarks.getByRole('button', { name: '打开收藏 /missing', exact: true }).waitFor();
      await bookmarks.getByRole('button', { name: '打开收藏 /missing', exact: true }).click();
      await page.getByText('无法打开 /missing：Permission denied', { exact: true }).waitFor();
      assert.equal(await page.getByRole('button', { name: 'Home', exact: true }).getAttribute('aria-pressed'), 'true');
      state.failPinWrites = true;
      await page.getByRole('button', { name: 'Pin 当前目录', exact: true }).click();
      await bookmarks.getByText('收藏保存失败：Config write failed', { exact: true }).waitFor();
      assert.equal(await bookmarks.getByRole('button', { name: `打开收藏 ${home}`, exact: true }).count(), 0);
      await bookmarks.getByRole('button', { name: '取消 Pin /missing', exact: true }).click();
      await bookmarks.getByText('收藏保存失败：Config write failed', { exact: true }).waitFor();
      assert.equal(await bookmarks.getByRole('button', { name: '打开收藏 /missing', exact: true }).count(), 1);
      state.failPinWrites = false;
      await bookmarks.getByRole('button', { name: '取消 Pin /missing', exact: true }).click();
      await bookmarks.getByText('Pin 常用目录，在这里快速访问。', { exact: true }).waitFor();
      assert.deepEqual(state.pins.paths, []);
      assert.deepEqual(state.errors, []);
    } finally { await context.close(); }
  });

  await test('out of order pin responses cannot remove a newer bookmark', async () => {
    const { context, page, state } = await fixture(browser);
    try {
      await openBrowser(page);
      await atPath(page, home);
      state.delayedPin = home;
      state.pinGate = deferred();
      const firstSave = page.waitForRequest((request) => request.method() === 'PATCH' && request.url().endsWith('/api/aow/pinned-directories') && request.postDataJSON().add?.includes(home));
      await page.getByRole('button', { name: 'Pin 当前目录', exact: true }).click();
      await firstSave;
      await page.locator('.system-file-list').getByRole('button', { name: 'Pin Documents', exact: true }).click();
      const bookmarks = page.getByRole('region', { name: '收藏目录', exact: true });
      await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).waitFor();
      state.pinGate.resolve();
      await page.waitForFunction(() => !document.querySelector('.system-file-toolbar .system-file-pin').disabled);
      assert.equal(await bookmarks.getByRole('button', { name: `打开收藏 ${home}/Documents`, exact: true }).count(), 1);
      assert.equal(await bookmarks.locator('li').count(), 2);
      assert.deepEqual(state.errors, []);
    } finally { state.pinGate?.resolve(); await context.close(); }
  });

  await test('Home failures can be retried without blocking other shortcuts', async () => {
    const { context, page, state } = await fixture(browser, { failHome: true });
    try {
      await openBrowser(page);
      await page.getByRole('alert').getByText('无法打开 Home：Home unavailable', { exact: true }).waitFor();
      await page.getByRole('button', { name: '/tmp', exact: true }).click();
      await atPath(page, '/tmp');
      state.failHome = false;
      await page.getByRole('button', { name: 'Home', exact: true }).click();
      await atPath(page, home);
      assert.deepEqual(state.errors, []);
    } finally { await context.close(); }
  });
} finally {
  await browser?.close();
  await new Promise((resolve) => server.httpServer.close(resolve));
}
