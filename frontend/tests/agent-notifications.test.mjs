import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const connections = new Set();
let preferences = { enabled: true, channels: ['page'] };
let providers = [];
let storedSecret;
let publicBaseUrl = "";
const settings = () => ({ im: { providers }, notifications: { agent_task_completed: preferences, public_base_url: publicBaseUrl } });
const server = await createServer({
  root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, proxy: {} },
  plugins: [{ name: 'task-stop-fixture', configureServer(server) {
    server.middlewares.use('/api/aow/notification-settings', async (request, response) => {
      response.setHeader('Content-Type', 'application/json');
      if (request.method === 'PUT') {
        let text = '';
        for await (const chunk of request) text += chunk;
        const update = JSON.parse(text);
        if (update.section === 'im') {
          if (!update.providers.length && preferences.channels.includes('feishu')) {
            response.statusCode = 400;
            response.end(JSON.stringify({ message: '请先取消选择飞书推送' }));
            return;
          }
          providers = update.providers.map(provider => {
            storedSecret = provider.app_secret || storedSecret;
            return { provider: 'feishu', app_id: provider.app_id, secret_configured: true };
          });
        } else { preferences = update.agent_task_completed; publicBaseUrl = update.public_base_url ?? publicBaseUrl; }
      }
      response.end(JSON.stringify(settings()));
    });
    server.middlewares.use('/api/aow/im/feishu', async (request, response) => {
      response.setHeader('Content-Type', 'application/json');
      if (request.method === 'DELETE') {
        if (preferences.channels.includes('feishu')) {
          response.statusCode = 400;
          response.end(JSON.stringify({ message: '请先取消选择飞书推送' }));
          return;
        }
        providers = providers.filter(provider => provider.provider !== 'feishu');
      } else {
        let text = '';
        for await (const chunk of request) text += chunk;
        const input = JSON.parse(text);
        storedSecret = input.app_secret || storedSecret;
        providers = [...providers.filter(provider => provider.provider !== 'feishu'),
          { provider: 'feishu', app_id: input.app_id, secret_configured: true }];
      }
      response.end(JSON.stringify(settings()));
    });
    server.middlewares.use('/api/terminals/task-stops', (request, response) => {
      response.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' });
      response.write(': connected\n\n');
      connections.add(response);
      response.on('close', () => connections.delete(response));
    });
    server.middlewares.use('/api/terminals/', (request, response) => {
      const id = decodeURIComponent(request.url.slice(1));
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify({ id, name: id, workspace_root: '/workspace/demo', revision: 1,
        layout: { type: 'pane', pane_id: 'pane' }, panes: [{ id: 'pane', name: id, cwd: '/workspace/demo', kind: 'terminal',
          shell: '/bin/bash', status: 'running', rows: 24, cols: 80 }] }));
    });
    server.middlewares.use('/api/aow/projects', (_request, response) => {
      response.setHeader('Content-Type', 'application/json');
      response.end(JSON.stringify([{ id: 'project', name: 'AoW', worktrees: [{ id: 'worktree', path: '/workspace/demo' }] }]));
    });
  } }],
});
let browser;
const source = (tab_id = 'tab', project_name = 'AoW', tab_name = '性能排查') => ({
  tab_id, project_name, tab_name, workspace_root: '/workspace/demo',
});
const notice = (id, agent = 'codex') => ({ agent, session_id: id, title: '同名任务', cwd: '/workspace/demo', instance_ids: ['pane'], sources: [source(id, 'AoW', id)] });
const notification = (page, tabName) => page.getByRole('button', { name: `打开通知：AoW · ${tabName}`, exact: true });
function send(data) {
  if (!preferences.enabled || !preferences.channels.includes('page')) return;
  for (const response of connections) response.write(`event: task-stopped\ndata: ${JSON.stringify(data)}\n\n`);
}
async function waitConnections(count) {
  const deadline = Date.now() + 10_000;
  while (connections.size !== count && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 25));
  assert.equal(connections.size, count);
}

try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  await test('live stops produce independent, dismissible notices without stealing focus', async t => {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    t.after(() => page.close());
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await waitConnections(1);
    await page.getByLabel('继续工作').focus();
    send({ ...notice('one'), conclusion: '仅飞书展示的本轮结论'.repeat(10000) });
    await notification(page, 'one').waitFor();
    assert.equal(await page.getByText('仅飞书展示的本轮结论', { exact: false }).count(), 0);
    assert.equal(await page.locator('.agent-task-notice-heading strong').textContent(), 'AoW');
    assert.equal(await page.locator('.agent-task-notice-tab').textContent(), 'one');
    assert.match(await page.locator('.agent-task-notice .agent-icon').getAttribute('src'), /codex\.png/);
    assert.equal(await page.locator('.agent-task-notice a').count(), 0);
    assert.equal(await page.locator('.agent-task-notice').getByRole('button').count(), 2);
    await page.getByRole('button', { name: '关闭通知：AoW · one', exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '移除通知' }).count(), 0);
    assert.equal(await page.getByText('同名任务', { exact: true }).count(), 0);
    assert.equal(await page.getByText('Session ID', { exact: false }).count(), 0);
    assert.equal(await page.getByLabel('继续工作').evaluate(element => document.activeElement === element), true);
    send(notice('two', 'traecli'));
    send(notice('one'));
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 3);
    assert.equal(await page.locator('.agent-task-notice-heading strong').count(), 3);
    await page.locator('.agent-task-notice-open').first().click({ position: { x: 5, y: 5 } });
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 2);
    assert.equal(await page.locator('.agent-task-notice').count(), 2);
    await page.getByRole('button', { name: '全部关闭' }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    assert.equal(await page.locator('.agent-task-notice').count(), 0);

    send({ invalid: true });
    send({ ...notice('00000000-0000-4000-8000-000000000001', 'claude'), title: '<img src=x onerror="window.bad=true">',
      sources: [source(), source('other-tab', 'TraeCode CLI', 'Terminal 34'), null, { invalid: true }] });
    await page.getByText('AoW、TraeCode CLI', { exact: true }).waitFor();
    await page.getByText('性能排查、Terminal 34', { exact: true }).waitFor();
    assert.equal(await page.locator('.agent-task-notice img:not(.agent-icon)').count(), 0);
    assert.match(await page.locator('.agent-task-notice .agent-icon').getAttribute('src'), /claude\.png/);
    assert.equal(await page.evaluate(() => window.bad), undefined);
    await page.setViewportSize({ width: 390, height: 844 });
    const box = await page.locator('.agent-task-notice').boundingBox();
    assert.ok(box.x >= 0 && box.x + box.width <= 390);
    await page.screenshot({ path: '/tmp/aow-agent-notifications.png' });

    // Older servers still produce a readable notice during a rolling upgrade.
    send({ ...notice('legacy'), sources: undefined });
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 2);
    assert.equal(await page.getByRole('button', { name: '打开通知：demo · Tab 不可用', exact: true }).isDisabled(), true);

    await page.getByRole('button', { name: '切换订阅' }).click();
    await waitConnections(0);
    send(notice('while-disconnected'));
    await page.getByRole('button', { name: '切换订阅' }).click();
    await waitConnections(1);
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 2);
    assert.equal(await page.getByText('while-disconnected', { exact: true }).count(), 0);
    send(notice('after-remount'));
    await page.getByText('after-remount', { exact: true }).waitFor();
    assert.deepEqual(errors, []);
    await page.close();
    await waitConnections(0);
  });
  await test('older notices fold into a persistent, scrollable stack and disappear permanently when dismissed', async t => {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    t.after(() => page.close());
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await waitConnections(1);
    for (let index = 1; index <= 8; index++) send(notice(`session-${index}`));
    await page.getByText('session-8', { exact: true }).waitFor();
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(),
      ['session-8', 'session-7', 'session-6', 'session-5', 'session-4']);
    await page.getByRole('button', { name: /还有 3 条通知/ }).waitFor();
    await page.screenshot({ path: '/tmp/aow-notifications-collapsed.png' });
    await page.setViewportSize({ width: 390, height: 600 });
    const stack = await page.getByRole('button', { name: /还有 3 条通知/ }).boundingBox();
    assert.ok(stack.x >= 0 && stack.x + stack.width <= 390 && stack.y >= 0 && stack.y + stack.height <= 600);
    await page.screenshot({ path: '/tmp/aow-notifications-mobile.png' });
    await page.setViewportSize({ width: 1280, height: 800 });
    await page.reload();
    await page.getByRole('button', { name: /还有 3 条通知/ }).click();
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 8);
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(),
      Array.from({ length: 8 }, (_, i) => `session-${8 - i}`));
    const list = page.getByLabel('待处理通知', { exact: true });
    await page.setViewportSize({ width: 1280, height: 500 });
    assert.equal(await list.evaluate(element => element.scrollHeight > element.clientHeight), true);
    await list.hover();
    await page.mouse.wheel(0, 700);
    await page.waitForFunction(() => document.querySelector('.agent-task-notifications-list').scrollTop > 0);
    assert.equal(await page.evaluate(() => window.scrollY), 0);
    await waitConnections(1);
    send(notice('session-9'));
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 9);
    await page.getByRole('button', { name: '收起', exact: true }).waitFor();
    await page.screenshot({ path: '/tmp/aow-notifications-expanded.png' });
    await notification(page, 'session-1').click();
    await page.getByText('session-1', { exact: true }).waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '收起', exact: true }).click();
    await page.getByRole('button', { name: /还有 3 条通知/ }).waitFor();
    await page.reload();
    await page.getByRole('button', { name: /还有 3 条通知/ }).click();
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 8);
    assert.equal(await page.getByText('session-1', { exact: true }).count(), 0);
    assert.equal(await page.getByText('session-9', { exact: true }).count(), 1);
    await page.getByRole('button', { name: '全部关闭' }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await page.reload();
    await page.getByLabel('继续工作').waitFor();
    await waitConnections(1);
    send(notice('session-9'));
    await page.getByText('session-9', { exact: true }).waitFor();
    assert.equal(await page.locator('.agent-task-notice').count(), 1);
    await page.close();
    await waitConnections(0);
  });
  await test('closing popups with the keyboard updates folded and expanded lists without reopening them on remount', async t => {
    const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
    t.after(() => page.close());
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await waitConnections(1);
    for (let index = 1; index <= 7; index++) send(notice(`close-${index}`));
    await page.getByRole('button', { name: /还有 2 条通知/ }).waitFor();
    const close = page.getByRole('button', { name: '关闭通知：AoW · close-7', exact: true });
    const cardBox = await page.locator('.agent-task-notice').first().boundingBox();
    const closeBox = await close.boundingBox();
    assert.ok(closeBox.x >= cardBox.x && closeBox.x + closeBox.width <= cardBox.x + cardBox.width);
    assert.ok(closeBox.y >= cardBox.y && closeBox.y + closeBox.height <= cardBox.y + cardBox.height);
    await page.screenshot({ path: '/tmp/aow-notification-close-mobile.png' });
    await close.press('Enter');
    await page.getByRole('button', { name: /还有 1 条通知/ }).waitFor();
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(),
      ['close-6', 'close-5', 'close-4', 'close-3', 'close-2']);
    await page.getByRole('button', { name: /还有 1 条通知/ }).click();
    await page.getByRole('button', { name: '关闭通知：AoW · close-6', exact: true }).press('Space');
    await notification(page, 'close-6').waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '收起', exact: true }).waitFor();
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(),
      ['close-5', 'close-4', 'close-3', 'close-2', 'close-1']);
    await page.getByRole('button', { name: '全部关闭', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await page.getByRole('button', { name: '切换订阅' }).click();
    await waitConnections(0);
    await page.getByRole('button', { name: '切换订阅' }).click();
    await waitConnections(1);
    send(notice('close-7'));
    await notification(page, 'close-7').waitFor();
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(), ['close-7']);
    await page.close();
    await waitConnections(0);
  });
  await test('the queue retains more than 200 notices without evicting old entries', async t => {
    const page = await browser.newPage();
    t.after(() => page.close());
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await waitConnections(1);
    for (let index = 1; index <= 205; index++) send(notice(`bulk-${index}`));
    await page.getByRole('button', { name: /还有 200 条通知/ }).waitFor();
    assert.equal(await page.locator('.agent-task-notice').count(), 5);
    await page.reload();
    await page.getByRole('button', { name: /还有 200 条通知/ }).click();
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 205);
    assert.equal(await page.getByText('bulk-1', { exact: true }).count(), 1);
    assert.equal(await page.getByText('bulk-205', { exact: true }).count(), 1);
    await page.getByRole('button', { name: '全部关闭' }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await page.reload();
    await page.getByLabel('继续工作').waitFor();
    await waitConnections(1);
    send(notice('after-clear'));
    await page.getByText('after-clear', { exact: true }).waitFor();
    assert.equal(await page.locator('.agent-task-notice').count(), 1);
    await page.close();
    await waitConnections(0);
  });
  await test('live notifications still work when browser storage is unavailable', async t => {
    const page = await browser.newPage();
    t.after(() => page.close());
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(() => Object.defineProperty(window, 'indexedDB', { value: { open() { throw new Error('Storage disabled'); } } }));
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await waitConnections(1);
    send(notice('memory-only'));
    await page.getByText('memory-only', { exact: true }).waitFor();
    await page.getByRole('alert').filter({ hasText: '通知暂时无法保存' }).waitFor();
    await page.getByRole('button', { name: '关闭通知：AoW · memory-only', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    send(notice('memory-only'));
    await notification(page, 'memory-only').click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    assert.deepEqual(errors, []);
    await page.close();
    await waitConnections(0);
  });
  await test('IM credentials and completion channels can be configured and persist across reload', async t => {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
    t.after(() => page.close());
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await waitConnections(1);
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page.getByLabel('Agent 任务完成', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('页面提示', { exact: true }).isChecked(), true);
    assert.equal(await page.getByLabel('飞书推送', { exact: true }).count(), 0);
    await page.getByLabel('页面提示', { exact: true }).uncheck();
    assert.equal(await page.getByRole('button', { name: '保存', exact: true }).isDisabled(), true);
    await page.getByText('至少选择一种通知方式。', { exact: true }).waitFor();
    await page.getByLabel('页面提示', { exact: true }).check();
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByLabel('飞书 App ID').fill('cli_test');
    await page.getByLabel('飞书 App Secret').fill('test-secret');
    assert.equal(await page.getByLabel('飞书 App Secret').getAttribute('type'), 'password');
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('IM 配置已保存。', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('飞书 App Secret').inputValue(), '');
    assert.equal(storedSecret, 'test-secret');
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page.getByLabel('飞书推送', { exact: true }).check();
    await page.getByRole('button', { name: '使用当前访问地址', exact: true }).click();
    assert.equal(await page.getByLabel('AoW 访问地址').inputValue(), base);
    await page.getByLabel('AoW 访问地址').fill('https://aow.example.com');
    await page.getByLabel('页面提示', { exact: true }).uncheck();
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('通知配置已保存，即时生效。', { exact: true }).waitFor();
    send(notice('im-only'));
    assert.equal(await page.locator('.agent-task-notice').count(), 0);
    await page.reload();
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page.getByLabel('飞书推送', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('飞书推送', { exact: true }).isChecked(), true);
    assert.equal(await page.getByLabel('AoW 访问地址').inputValue(), 'https://aow.example.com');
    assert.equal(await page.getByLabel('页面提示', { exact: true }).isChecked(), false);
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByRole('button', { name: '移除飞书配置', exact: true }).click();
    await page.getByText('请先取消选择飞书推送', { exact: true }).waitFor();
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page.getByLabel('页面提示', { exact: true }).check();
    await page.getByLabel('飞书推送', { exact: true }).uncheck();
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('通知配置已保存，即时生效。', { exact: true }).waitFor();
    send(notice('page-enabled'));
    await notification(page, 'page-enabled').click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await page.getByLabel('Agent 任务完成', { exact: true }).uncheck();
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('通知配置已保存，即时生效。', { exact: true }).waitFor();
    send(notice('disabled'));
    assert.equal(await page.locator('.agent-task-notice').count(), 0);
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByRole('button', { name: '移除飞书配置', exact: true }).click();
    await page.getByText('IM 配置已保存。', { exact: true }).waitFor();
    assert.equal(await page.getByLabel('飞书 App ID').inputValue(), '');
    await page.screenshot({ path: '/tmp/aow-im-settings.png' });
    await page.close();
    await waitConnections(0);
  });
  await test('wechat QR verification, private test delivery and provider edits work independently', async t => {
    preferences = { enabled: true, channels: ['page'] };
    providers = [{ provider: 'feishu', app_id: 'cli_existing', secret_configured: true }];
    const page = await browser.newPage({ viewport: { width: 1100, height: 1000 } });
    t.after(() => page.close());
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    let testsSent = 0;
    const qr = `data:image/svg+xml;base64,${Buffer.from('<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256"><rect width="256" height="256" fill="white"/><rect x="32" y="32" width="64" height="64"/></svg>').toString('base64')}`;
    await page.route('**/api/aow/im/wechat**', async route => {
      const request = route.request();
      const path = new URL(request.url()).pathname;
      if (path.endsWith('/login')) return route.fulfill({ json: { id: 'qr-1', status: 'wait', qr_image: qr, message: '请用手机微信扫码。' } });
      if (path.endsWith('/login/qr-1')) {
        if (request.method() === 'DELETE') return route.fulfill({ status: 204 });
        const code = request.postDataJSON()?.verify_code;
        if (!code) return route.fulfill({ json: { id: 'qr-1', status: 'need_verifycode', qr_image: qr, message: '请输入手机微信显示的数字。' } });
        assert.equal(code, '123456');
        providers.push({ provider: 'wechat', account_id: 'bot', user_id: 'scanner' });
        return route.fulfill({ json: { id: 'qr-1', status: 'confirmed', qr_image: null, message: '微信 Bot 已绑定，通知将发给本次扫码的微信账号。' } });
      }
      if (path.endsWith('/test')) { testsSent++; return route.fulfill({ json: { message: '测试消息已发送。' } }); }
      if (request.method() === 'DELETE') {
        if (preferences.channels.includes('wechat')) return route.fulfill({ status: 400, json: { message: '移除机器人前请取消选择微信推送' } });
        providers = providers.filter(provider => provider.provider !== 'wechat');
        return route.fulfill({ json: settings() });
      }
      return route.fulfill({ json: { connection: { receiving: true, context_ready: true, error: null } } });
    });
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByRole('button', { name: '扫码连接微信' }).click();
    await page.getByLabel('微信配对码').waitFor();
    assert.equal(await page.getByAltText('微信 Bot 登录二维码').evaluate(image => image.complete && image.naturalWidth > 0), true);
    await page.screenshot({ path: '/tmp/aow-wechat-pairing.png' });
    await page.getByLabel('微信配对码').fill('123456');
    await page.getByRole('button', { name: '确认配对码' }).click();
    await page.getByText('接收账号：scanner', { exact: true }).waitFor();
    assert.equal(testsSent, 0);
    await page.getByRole('button', { name: '发送微信测试消息' }).click();
    await page.getByText('测试消息已发送。', { exact: true }).waitFor();
    assert.equal(testsSent, 1);
    await page.getByLabel('飞书 App ID').fill('cli_new');
    await page.getByLabel('飞书 App Secret').fill('secret');
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('IM 配置已保存。', { exact: true }).waitFor();
    assert.equal(providers.some(provider => provider.provider === 'wechat'), true);
    await page.getByRole('button', { name: '移除飞书配置' }).click();
    await page.getByRole('button', { name: '移除飞书配置' }).waitFor({ state: 'detached' });
    assert.deepEqual(providers.map(provider => provider.provider), ['wechat']);
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page.getByLabel('微信推送', { exact: true }).check();
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('通知配置已保存，即时生效。', { exact: true }).waitFor();
    assert.deepEqual(preferences.channels, ['page', 'wechat']);
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByRole('button', { name: '解除微信绑定' }).click();
    await page.getByText('移除机器人前请取消选择微信推送', { exact: true }).waitFor();
    assert.equal(providers.length, 1);
    await page.getByRole('button', { name: '通知设置', exact: true }).click();
    await page.getByLabel('微信推送', { exact: true }).uncheck();
    await page.getByRole('button', { name: '保存', exact: true }).click();
    await page.getByText('通知配置已保存，即时生效。', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByRole('button', { name: '解除微信绑定' }).click();
    await page.getByRole('button', { name: '扫码连接微信' }).waitFor();
    assert.deepEqual(providers, []);
    assert.deepEqual(errors, []);
  });

  await test('expired QR can be refreshed and leaving settings cancels pending login', async t => {
    preferences = { enabled: true, channels: ['page'] }; providers = [];
    const page = await browser.newPage({ viewport: { width: 390, height: 844 } });
    t.after(() => page.close());
    let starts = 0;
    const cancelled = [];
    await page.route('**/api/aow/im/wechat/login**', route => {
      const request = route.request();
      if (request.url().endsWith('/login')) return route.fulfill({ json: { id: `qr-${++starts}`, status: 'wait', qr_image: null, message: '等待扫码' } });
      if (request.method() === 'DELETE') { cancelled.push(request.url().split('/').at(-1)); return route.fulfill({ status: 204 }); }
      const id = request.url().split('/').at(-1);
      return route.fulfill({ json: { id, status: id === 'qr-1' ? 'expired' : 'wait', qr_image: null, message: id === 'qr-1' ? '二维码已过期，请重新生成。' : '等待扫码' } });
    });
    await page.goto(`${base}/tests/agent-notifications-preview.html`);
    await page.getByRole('button', { name: 'IM 设置', exact: true }).click();
    await page.getByRole('button', { name: '扫码连接微信' }).click();
    await page.getByText('二维码已过期，请重新生成。', { exact: true }).waitFor();
    await page.getByRole('button', { name: '重新生成二维码' }).click();
    await page.getByText('等待扫码', { exact: true }).waitFor();
    await Promise.all([
      page.waitForResponse(response => response.url().endsWith('/login/qr-2') && response.request().method() === 'DELETE'),
      page.getByRole('button', { name: '通知设置', exact: true }).click(),
    ]);
    assert.ok(cancelled.includes('qr-1') && cancelled.includes('qr-2'));
  });
} finally {
  await browser?.close();
  for (const response of connections) response.end();
  await server.close();
}
