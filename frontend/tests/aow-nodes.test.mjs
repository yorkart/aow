import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  const { parseNodeAddresses, otherNodeAddresses } = await server.ssrLoadModule('/src/aow/aowNodes.ts');
  await test('node list normalization preserves destinations and filters the current deployment', () => {
    const addresses = parseNodeAddresses(' HTTPS://NODE-A.EXAMPLE:443 \r\n\nhttps://node-a.example/\nhttps://node-a.example:8443/aow/?ui=desktop#tab\nhttp://node-a.example\nhttp://[::1]:8080');
    assert.deepEqual(addresses, ['https://node-a.example/', 'https://node-a.example:8443/aow/?ui=desktop#tab', 'http://node-a.example/', 'http://[::1]:8080/']);
    assert.deepEqual(otherNodeAddresses(addresses, 'https://NODE-A.example:443'), addresses.slice(1));
    assert.deepEqual(otherNodeAddresses(['http://[::1]:8080/aow', 'http://[::1]:8081', 'javascript:alert(1)'], 'http://[::1]:8080'), ['http://[::1]:8081/']);
    assert.deepEqual(parseNodeAddresses(' \n\r\n'), []);
    assert.deepEqual(otherNodeAddresses([
      'https://example.com/tools/aow/', 'https://example.com/tools/aow/aow/?ui=desktop',
      'https://example.com/tools/aow/m/', 'https://example.com/tools/other/', 'https://example.com/',
    ], 'https://example.com/tools/aow'), ['https://example.com/tools/other/', 'https://example.com/']);
    for (const invalid of ['node.example', '//node.example', 'https:node.example', 'javascript:alert(1)', 'file:///tmp/node', 'https://user:password@node.example']) {
      assert.throws(() => parseNodeAddresses(`https://valid.example\n\n${invalid}`), /第 3 行地址无效/);
    }
  });

  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function fixture(t, addresses, { mobile = false, width = 390 } = {}) {
    const context = await browser.newContext(mobile
      ? { viewport: { width, height: 844 }, isMobile: true, hasTouch: true }
      : { viewport: { width: 1280, height: 800 } });
    const state = { settings: { notes_base: '/notes', execution_path: ['/usr/bin'], editor: { word_wrap: false }, ...(addresses ? { node_addresses: addresses } : {}) }, updates: [], errors: [], failSave: false };
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); });
    await context.route('**/api/**', async route => {
      const request = route.request();
      const { pathname } = new URL(request.url());
      let data = [];
      if (pathname === '/api/auth/status') data = { configured: true, authenticated: true };
      else if (pathname === '/api/aow/settings') {
        if (request.method() === 'PUT') {
          state.updates.push(request.postDataJSON());
          if (state.failSave) { await route.fulfill({ status: 500, json: { message: 'Settings write failed' } }); return; }
          Object.assign(state.settings, request.postDataJSON());
        }
        data = state.settings;
      } else if (pathname.includes('/pinned-')) data = { paths: [], revision: 0 };
      else if (pathname === '/api/terminals/task-stops') { await route.fulfill({ contentType: 'text/event-stream', body: ': ready\n\n' }); return; }
      else if (pathname === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
      await route.fulfill({ json: data });
    });
    const page = await context.newPage();
    page.on('pageerror', error => state.errors.push(error.message));
    await page.goto(mobile ? `${base}/m` : `${base}/aow/?ui=desktop`);
    await page.getByRole('button', { name: '切换 AoW 节点', exact: true }).waitFor();
    return { page, state };
  }
  const trigger = page => page.getByRole('button', { name: '切换 AoW 节点', exact: true });
  const menu = page => page.getByRole('menu', { name: '其他 AoW 节点' });
  async function openSettings(page) {
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '设置', exact: true });
    await dialog.getByRole('button', { name: /^Nodes/ }).click();
    return dialog;
  }

  await test('save and reload a shared node list, filter self, and open another node in a new page', async t => {
    const { page, state } = await fixture(t);
    const destination = 'https://node-b.example/aow/?from=switcher#target';
    const otherPort = `http://127.0.0.1:${new URL(base).port === '8081' ? '8082' : '8081'}/`;
    let dialog = await openSettings(page);
    await dialog.getByRole('textbox', { name: '节点地址', exact: true }).fill(` ${base}/aow/ \n\n${destination}\n${destination}\n${otherPort}`);
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.updates, [{ node_addresses: [`${base}/aow/`, destination, otherPort] }]);
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    // Clicking the logo and the text both opens the same menu.
    await trigger(page).locator('img').click();
    assert.deepEqual(await menu(page).getByRole('menuitem').evaluateAll(items => items.map(item => item.href)), [destination, otherPort]);
    await page.keyboard.press('ArrowDown');
    assert.equal(await menu(page).getByRole('menuitem').nth(1).evaluate(item => item === document.activeElement), true);
    await page.keyboard.press('Escape');
    await menu(page).waitFor({ state: 'hidden' });
    assert.equal(await trigger(page).evaluate(item => item === document.activeElement), true);
    await trigger(page).getByText('AoW', { exact: true }).click();
    await page.locator('.project-aow-no-context').click();
    await menu(page).waitFor({ state: 'hidden' });
    await page.reload();
    dialog = await openSettings(page);
    assert.equal(await dialog.getByRole('textbox', { name: '节点地址', exact: true }).inputValue(), state.settings.node_addresses.join('\n'));
    await page.screenshot({ path: '/tmp/aow-node-settings.png' });
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await trigger(page).click();
    await page.screenshot({ path: '/tmp/aow-node-menu.png' });
    await page.context().route('https://node-b.example/**', route => route.fulfill({ contentType: 'text/html', body: '<h1>Other AoW</h1>' }));
    const originalUrl = page.url();
    const newPage = page.context().waitForEvent('page');
    await menu(page).getByRole('menuitem', { name: destination, exact: true }).click();
    const opened = await newPage;
    await opened.waitForURL(destination);
    assert.equal(await opened.getByRole('heading').innerText(), 'Other AoW');
    assert.equal(await opened.evaluate(() => window.opener), null);
    assert.equal(page.url(), originalUrl);
    await menu(page).waitFor({ state: 'hidden' });
  });

  await test('invalid input and failed saves retain the previous list; blank input clears it', async t => {
    const destination = 'https://node-b.example/';
    const { page, state } = await fixture(t, [destination]);
    let dialog = await openSettings(page);
    const textarea = dialog.getByRole('textbox', { name: '节点地址', exact: true });
    await textarea.fill('javascript:alert(1)');
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '第 1 行地址无效' }).waitFor();
    assert.deepEqual(state.updates, []);
    state.failSave = true;
    await textarea.fill('https://node-c.example/');
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: 'Settings write failed' }).waitFor();
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await trigger(page).click();
    assert.equal(await menu(page).getByRole('menuitem').getAttribute('href'), destination);
    await page.keyboard.press('Escape');
    state.failSave = false;
    dialog = await openSettings(page);
    await dialog.getByRole('textbox', { name: '节点地址', exact: true }).fill(' \n\n');
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.updates.at(-1), { node_addresses: [] });
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await trigger(page).click();
    await menu(page).getByText(/暂无其他节点/).waitFor();
    assert.equal(await menu(page).getByRole('menuitem').count(), 0);
  });

  await test('mobile home aligns the brand in one row and opens other nodes in a new page', async t => {
    const destination = 'https://node-b.example/aow/?from=mobile#target';
    for (const width of [320, 390]) {
      const { page } = await fixture(t, [`${base}/`, `${base}/m`, destination], { mobile: true, width });
      const brand = trigger(page);
      const logo = await brand.locator('img').boundingBox();
      const title = await brand.locator('strong').boundingBox();
      const subtitle = await brand.getByText('移动工作台', { exact: true }).boundingBox();
      assert.ok(logo.x + logo.width <= title.x && title.x + title.width <= subtitle.x, 'logo, title and subtitle appear in order on one row');
      assert.equal(title.height, logo.height, 'AoW and the logo have the same height');
      assert.equal(await brand.getByText('移动工作台', { exact: true }).evaluate(element => getComputedStyle(element).fontSize), '10px');
      assert.ok(Math.abs(subtitle.y + subtitle.height / 2 - logo.y - logo.height / 2) < 1, 'subtitle stays vertically centered');
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
      await brand.locator('img').tap();
      await menu(page).getByRole('menuitem', { name: destination, exact: true }).waitFor();
      assert.deepEqual(await menu(page).getByRole('menuitem').evaluateAll(items => items.map(item => item.href)), [destination]);
      const bounds = await menu(page).boundingBox();
      assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= width, 'node menu fits narrow phone screens');
      assert.ok((await menu(page).getByRole('menuitem').boundingBox()).height >= 44, 'node links have touch-sized targets');
      await page.screenshot({ path: `/tmp/aow-node-mobile-${width}.png` });
      await page.getByRole('heading', { name: '你的项目', exact: true }).tap();
      await menu(page).waitFor({ state: 'hidden' });
      await brand.getByText('AoW', { exact: true }).tap();
      await page.context().route('https://node-b.example/**', route => route.fulfill({ contentType: 'text/html', body: '<h1>Other AoW</h1>' }));
      const originalUrl = page.url();
      const newPage = page.context().waitForEvent('page');
      await menu(page).getByRole('menuitem', { name: destination, exact: true }).tap();
      const opened = await newPage;
      await opened.waitForURL(destination);
      assert.equal(await opened.getByRole('heading').innerText(), 'Other AoW');
      assert.equal(page.url(), originalUrl);
      await menu(page).waitFor({ state: 'hidden' });
    }
  });

  await test('old settings and a list containing only self show an empty menu on desktop and mobile', async t => {
    for (const mobile of [false, true]) for (const addresses of [undefined, [`${base}/`, `${base}/aow/?ui=desktop`]]) {
      const { page } = await fixture(t, addresses, { mobile });
      await trigger(page).click();
      await menu(page).getByText(/暂无其他节点/).waitFor();
      assert.equal(await menu(page).getByRole('menuitem').count(), 0);
    }
  });
} finally {
  await browser?.close();
  await server.close();
}
