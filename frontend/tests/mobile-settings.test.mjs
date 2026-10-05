import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir } from 'node:fs/promises';
import { chromium } from 'playwright';
import { productionPreview } from './fixtures/production-preview.mjs';

const server = await productionPreview();
const base = `http://127.0.0.1:${server.httpServer.address().port}`;
const browser = await chromium.launch({ headless: true, ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
const versions = ['550e8400-e29b-41d4-a716-446655440000', '550e8400-e29b-41d4-a716-446655440001'];
const categories = ['Configuration', 'Nodes', 'Editor', 'Notes', 'Environment', 'Agents', 'IM', '通知', 'Pull Requests'];

async function fixture(t, width = 390, mobile = true, virtualViewport = false) {
  const context = await browser.newContext({ viewport: { width, height: 844 }, isMobile: mobile, hasTouch: mobile });
  if (virtualViewport) await context.addInitScript(() => {
    Object.defineProperty(window, 'visualViewport', { value: Object.assign(new EventTarget(), { height: 844, width: 390, offsetTop: 0 }) });
  });
  const state = {
    errors: [], writes: [], failSave: false, failEnvironmentRead: false, holdSave: undefined,
    serverEnvironment: { path: '/home/aow/.config/aow/server.env', content: '# Existing comment\nAOW_SERVER_PORT=8282\n', revision: 'first', exists: true, platform: 'linux' },
    settings: { notes_base: '/notes', execution_path: ['/usr/bin', '/bin'], node_addresses: [], editor: { word_wrap: false } },
    agents: [{ id: 'codex', agent_type: 'codex', display_name: 'Codex', source: 'detected', available: true, command: 'codex', executable: '/usr/bin/codex', args: [], env: {} }],
    notifications: { im: { providers: [{ provider: 'wechat', account_id: 'account', user_id: 'user' }] }, notifications: { agent_task_completed: { enabled: true, channels: ['page'] }, public_base_url: '' } },
    configuration: { config_repo: '/repo', config_id: versions[0] },
    providers: { providers: [{ id: 'github', name: 'GitHub', hosts: ['github.com'], enabled: true, script: 'print("describe")' }] },
  };
  t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); });
  await context.route('**/api/**', async route => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const writing = request.method() !== 'GET';
    const body = request.postDataJSON();
    if (writing) {
      state.writes.push({ path, method: request.method(), body });
      if (state.holdSave) await state.holdSave;
      if (state.failSave) { await route.fulfill({ status: 500, json: { message: '配置写入失败' } }); return; }
    }
    let data = [];
    if (path === '/api/auth/status') data = { configured: true, authenticated: true };
    else if (path === '/api/aow/settings') {
      if (writing) Object.assign(state.settings, body);
      data = state.settings;
    } else if (path === '/api/aow/settings/server-environment') {
      if (!writing && state.failEnvironmentRead) {
        await route.fulfill({ status: 403, json: { message: 'server.env 读取失败' } }); return;
      }
      if (writing) {
        if (body.revision !== state.serverEnvironment.revision) {
          await route.fulfill({ status: 409, json: { message: 'server.env 已被外部程序修改，请重新读取文件后再保存。' } }); return;
        }
        Object.assign(state.serverEnvironment, { content: body.content, revision: `${state.writes.length}`, exists: true });
      }
      data = state.serverEnvironment;
    } else if (path === '/api/aow/settings/discovered-path') data = ['/opt/bin', '/usr/bin', '/bin'];
    else if (path === '/api/aow/agents') {
      if (writing) {
        const agent = { ...body, id: body.id || `custom-agent-${state.agents.length}`, available: true, source: 'configured', executable: body.command };
        state.agents = [...state.agents.filter(item => item.id !== agent.id), agent];
        data = agent;
      } else data = state.agents;
    } else if (path.startsWith('/api/aow/agents/') && request.method() === 'DELETE') {
      state.agents = state.agents.filter(agent => agent.id !== path.split('/').at(-1));
      await route.fulfill({ status: 204 }); return;
    } else if (path === '/api/aow/settings/configuration') {
      if (writing) state.configuration = body;
      data = { selection: state.configuration, active_selection: { config_repo: '/repo', config_id: versions[0] }, restart_required: state.configuration.config_id !== versions[0], changed: writing };
    } else if (path === '/api/aow/settings/configuration/versions') data = { config_repo: url.searchParams.get('path'), config_ids: versions, selected_id: null };
    else if (path === '/api/aow/review-providers') {
      if (writing) state.providers = body;
      data = state.providers;
    } else if (path === '/api/aow/review-providers/test') data = { operations: ['list', 'detail', 'diff'] };
    else if (path === '/api/aow/im/feishu') {
      state.notifications.im.providers = state.notifications.im.providers.filter(provider => provider.provider !== 'feishu');
      if (request.method() === 'PUT') state.notifications.im.providers.push({ provider: 'feishu', app_id: body.app_id });
      data = state.notifications;
    } else if (path === '/api/aow/im/wechat') data = { connection: { context_ready: true } };
    else if (path === '/api/aow/notification-settings') {
      if (writing) state.notifications.notifications = { agent_task_completed: body.agent_task_completed, public_base_url: body.public_base_url };
      data = state.notifications;
    } else if (path.includes('/pinned-')) data = { paths: [], revision: 0 };
    else if (path === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
    else if (path === '/api/terminals/task-stops') { await route.fulfill({ contentType: 'text/event-stream', body: ': ready\n\n' }); return; }
    else if (writing) throw new Error(`Unexpected write: ${request.method()} ${path}`);
    await route.fulfill({ json: data });
  });
  const page = await context.newPage();
  page.setDefaultTimeout(10000);
  page.on('pageerror', error => state.errors.push(error.message));
  await page.goto(`${base}/${mobile ? 'm' : 'aow/?ui=desktop'}`);
  const trigger = page.getByRole('button', { name: mobile ? '设置' : 'Settings', exact: true });
  await trigger.waitFor();
  if (mobile) {
    const logout = await page.getByRole('button', { name: '退出登录', exact: true }).boundingBox();
    const settings = await trigger.boundingBox();
    assert.ok(logout.x >= settings.x + settings.width, 'Logout sits immediately to the right of Settings');
  }
  await trigger.click();
  const dialog = page.getByRole('dialog');
  await dialog.getByRole('navigation', { name: '设置分类' }).waitFor();
  const choose = async name => {
    if (mobile && await dialog.getByRole('button', { name: '返回设置分类' }).isVisible()) await dialog.getByRole('button', { name: '返回设置分类' }).click();
    await dialog.getByRole('navigation', { name: '设置分类' }).getByRole('button', { name: new RegExp(`^${name}`) }).click();
  };
  const save = async text => {
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    if (text) await dialog.getByRole('status').filter({ hasText: text }).waitFor();
  };
  return { page, state, dialog, choose, save, trigger };
}

async function layout(page, dialog, name) {
  assert.deepEqual(await dialog.evaluate(element => {
    const content = element.querySelector('.project-aow-settings-content');
    const bodies = [...content.querySelectorAll('.project-aow-dialog-body')].filter(body => body.getClientRects().length);
    return [element.scrollWidth <= element.clientWidth, content.scrollWidth <= content.clientWidth,
      bodies.every(body => body.scrollWidth <= body.clientWidth)];
  }), [true, true, true], `${name} fits the screen`);
  const footer = dialog.locator('.project-aow-dialog-footer:visible');
  if (await footer.count()) {
    const bounds = await footer.boundingBox();
    assert.ok(bounds.y + bounds.height <= page.viewportSize().height + 1, `${name} save actions stay visible`);
  }
  if (process.env.MOBILE_TEST_SCREENSHOTS) {
    await mkdir(process.env.MOBILE_TEST_SCREENSHOTS, { recursive: true });
    if (name.startsWith('agents')) await dialog.locator('.project-aow-agent-list').scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${process.env.MOBILE_TEST_SCREENSHOTS}/settings-${page.viewportSize().width}-${name}.png` });
  }
}

try {
  await test('server.env read failures can be retried and macOS files retain CRLF line endings', async t => {
    const { page, state, dialog, choose } = await fixture(t);
    state.failEnvironmentRead = true;
    state.serverEnvironment.platform = 'macos';
    state.serverEnvironment.content = '# Comment\r\nAOW_SERVER_PORT=8282\r\n';
    await choose('Environment');
    await dialog.getByRole('alert').filter({ hasText: 'server.env 读取失败' }).waitFor();
    assert.equal(await dialog.getByRole('button', { name: '保存 server.env', exact: true }).isDisabled(), true);
    state.failEnvironmentRead = false;
    await dialog.getByRole('button', { name: '重新读取文件', exact: true }).click();
    await dialog.getByText(/LaunchDaemon 模式需按安装器提示/).waitFor();
    const editor = dialog.getByRole('textbox', { name: 'server.env 文件内容', exact: true });
    await page.waitForFunction(() => document.querySelector('textarea[aria-label="server.env 文件内容"]')?.value.includes('8282'));
    await editor.fill('# Comment\nAOW_SERVER_PORT=8283\n');
    await dialog.getByRole('button', { name: '保存 server.env', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: 'server.env 已保存' }).waitFor();
    assert.equal(state.serverEnvironment.content, '# Comment\r\nAOW_SERVER_PORT=8283\r\n');
  });

  for (const mobile of [false, true]) await test(`${mobile ? 'mobile' : 'desktop'} server.env edits preserve drafts and save the file independently of PATH`, async t => {
    const { page, state, dialog, choose, trigger } = await fixture(t, mobile ? 320 : 1280, mobile);
    const close = () => dialog.getByRole('button', { name: mobile ? '关闭设置' : '关闭', exact: true });
    await choose('Environment');
    const editor = dialog.getByRole('textbox', { name: 'server.env 文件内容', exact: true });
    await page.waitForFunction(() => document.querySelector('textarea[aria-label="server.env 文件内容"]')?.value.includes('8282'));
    await dialog.getByText('systemctl --user restart aow-server.service', { exact: true }).waitFor();
    const content = '# Keep comment\n\nAOW_SERVER_PORT="8283"\nCUSTOM=$HOME=a=b\n';
    await editor.fill(content);
    await choose('Editor');
    await choose('Environment');
    assert.equal(await editor.inputValue(), content, 'category changes retain drafts');
    page.once('dialog', prompt => prompt.dismiss());
    await close().click();
    assert.equal(await editor.inputValue(), content, 'cancelled close retains draft');
    state.failSave = true;
    await dialog.getByRole('button', { name: '保存 server.env', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '配置写入失败' }).waitFor();
    assert.equal(await editor.inputValue(), content);
    assert.equal(state.serverEnvironment.content.includes('8282'), true);
    state.failSave = false;
    await dialog.getByRole('button', { name: '保存 server.env', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: 'server.env 已保存' }).waitFor();
    assert.equal(state.serverEnvironment.content, content);
    assert.deepEqual(state.writes.at(-1).body, { content, revision: 'first' });
    assert.deepEqual(state.settings.execution_path, ['/usr/bin', '/bin']);
    await layout(page, dialog, 'server-environment');
    await close().click();
    await trigger.click();
    await choose('Environment');
    await page.waitForFunction(() => document.querySelector('textarea[aria-label="server.env 文件内容"]')?.value.includes('8283'));
    await editor.fill('LOCAL=draft\n');
    state.serverEnvironment.content = 'EXTERNAL=updated\n';
    state.serverEnvironment.revision = 'external';
    await dialog.getByRole('button', { name: '保存 server.env', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '已被外部程序修改' }).waitFor();
    assert.equal(await editor.inputValue(), 'LOCAL=draft\n');
    page.once('dialog', prompt => prompt.dismiss());
    await dialog.getByRole('button', { name: '重新读取文件', exact: true }).click();
    assert.equal(await editor.inputValue(), 'LOCAL=draft\n');
    page.once('dialog', prompt => prompt.accept());
    await dialog.getByRole('button', { name: '重新读取文件', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('textarea[aria-label="server.env 文件内容"]')?.value === 'EXTERNAL=updated\n');
    await editor.fill('');
    await dialog.getByRole('button', { name: '保存 server.env', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: 'server.env 已保存' }).waitFor();
    assert.equal(state.serverEnvironment.content, '', 'blank content clears the file');
  });

  for (const width of [320, 390]) await test(`mobile ${width}px exposes and saves every desktop settings category`, async t => {
    const { page, state, dialog, choose, save, trigger } = await fixture(t, width);
    assert.deepEqual(await dialog.locator('nav strong').allTextContents(), categories);
    await layout(page, dialog, 'overview');
    await choose('Configuration');
    await dialog.getByRole('radio', { name: versions[1], exact: true }).check();
    await save('重启服务后生效');
    assert.equal(state.configuration.config_id, versions[1]);
    await layout(page, dialog, 'configuration');

    await choose('Nodes');
    await dialog.getByRole('textbox', { name: '节点地址', exact: true }).fill('https://phone-node.example.com');
    await save('节点地址已保存');
    assert.deepEqual(state.settings.node_addresses, ['https://phone-node.example.com/']);
    await layout(page, dialog, 'nodes');

    await choose('Editor');
    await dialog.getByRole('checkbox', { name: /Word Wrap/ }).check();
    await save('已保存 Editor 配置');
    assert.equal(state.settings.editor.word_wrap, true);
    await layout(page, dialog, 'editor');

    await choose('Notes');
    const notes = dialog.getByRole('textbox', { name: 'Notes root' });
    assert.equal(await notes.evaluate(element => element === document.activeElement), false, 'opening a category does not pop up the keyboard');
    await notes.fill('/notes/mobile');
    await save();
    await dialog.locator('code').filter({ hasText: '/notes/mobile' }).waitFor();
    assert.equal(state.settings.notes_base, '/notes/mobile');
    await layout(page, dialog, 'notes');

    await choose('Environment');
    await dialog.getByRole('button', { name: '从本机环境读取' }).click();
    await page.waitForFunction(() => document.querySelector('textarea[aria-label="PATH 目录"]')?.value.startsWith('/opt/bin'));
    await dialog.getByRole('button', { name: '保存 PATH', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '执行环境已保存' }).waitFor();
    assert.deepEqual(state.settings.execution_path, ['/opt/bin', '/usr/bin', '/bin']);
    await layout(page, dialog, 'environment');

    await choose('Agents');
    await dialog.getByRole('combobox', { name: 'Agent 类型' }).selectOption('codex');
    assert.equal(await dialog.getByRole('textbox', { name: 'Executable' }).inputValue(), '/usr/bin/codex');
    await dialog.getByRole('textbox', { name: 'Display name' }).fill('手机 Codex');
    await dialog.getByRole('textbox', { name: 'Executable' }).fill('/usr/bin/codex');
    await dialog.getByRole('textbox', { name: 'Arguments', exact: true }).fill('--model\ngpt-6');
    await dialog.getByRole('textbox', { name: 'Environment variables' }).fill('MODE=mobile');
    await dialog.getByRole('button', { name: '注册', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '手机 Codex 配置已保存' }).waitFor();
    assert.deepEqual(state.agents.at(-1).args, ['--model', 'gpt-6']);
    assert.deepEqual(state.agents.at(-1).env, { MODE: 'mobile' });
    await layout(page, dialog, 'agents');

    const original = structuredClone(state.agents.at(-1));
    const copy = dialog.getByRole('button', { name: '复制 手机 Codex', exact: true });
    assert.equal(await copy.evaluate(element => element.getBoundingClientRect().width), 44);
    await copy.click();
    assert.equal(await dialog.getByRole('textbox', { name: 'Display name' }).inputValue(), '手机 Codex（副本）');
    assert.equal(await dialog.getByRole('textbox', { name: 'Arguments', exact: true }).inputValue(), '--model\ngpt-6');
    assert.equal(await dialog.getByRole('textbox', { name: 'Environment variables' }).inputValue(), 'MODE=mobile');
    page.once('dialog', prompt => prompt.dismiss());
    await dialog.getByRole('button', { name: '关闭设置' }).click();
    assert.equal(await dialog.getByRole('textbox', { name: 'Display name' }).inputValue(), '手机 Codex（副本）', 'copied drafts retain unsaved-change protection');
    await dialog.getByRole('textbox', { name: 'Environment variables' }).fill('MODE=copied');
    await dialog.getByRole('button', { name: '注册', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '手机 Codex（副本） 配置已保存' }).waitFor();
    assert.equal(state.agents.length, 3);
    assert.deepEqual(state.agents.find(agent => agent.id === original.id), original);
    assert.deepEqual(state.agents.at(-1).env, { MODE: 'copied' });
    assert.equal(Object.hasOwn(state.writes.at(-1).body, 'id'), false);
    await layout(page, dialog, 'agents-copy');

    await choose('IM');
    await dialog.getByRole('button', { name: '重新扫码绑定' }).waitFor();
    await dialog.getByRole('textbox', { name: '飞书 App ID' }).fill('cli_mobile');
    await dialog.getByLabel('飞书 App Secret').fill('fixture-secret');
    await save('IM 配置已保存');
    await layout(page, dialog, 'im');

    await choose('通知');
    await dialog.getByRole('checkbox', { name: '微信推送' }).check();
    await dialog.getByRole('checkbox', { name: '飞书推送' }).check();
    await dialog.getByRole('textbox', { name: 'AoW 访问地址' }).fill('https://phone.example.com/aow/');
    await save('通知配置已保存');
    assert.deepEqual(state.notifications.notifications.agent_task_completed.channels, ['page', 'wechat', 'feishu']);
    await layout(page, dialog, 'notifications');

    await choose('Pull Requests');
    await dialog.getByRole('textbox', { name: 'Provider 显示名称' }).fill('Mobile GitHub');
    await dialog.getByLabel('上传 Provider 脚本').setInputFiles({ name: 'provider.py', mimeType: 'text/x-python', buffer: Buffer.from('print("mobile provider")') });
    await dialog.getByRole('status').filter({ hasText: '已读取文件内容' }).waitFor();
    await dialog.getByRole('button', { name: '检查协议' }).click();
    await dialog.getByRole('status').filter({ hasText: '协议检查通过' }).waitFor();
    await save('Provider 配置和脚本已保存');
    assert.equal(state.providers.providers[0].script, 'print("mobile provider")');
    await layout(page, dialog, 'pull-requests');
    await page.setViewportSize({ width: 844, height: 390 });
    await page.waitForFunction(() => document.querySelector('.mobile-app').getBoundingClientRect().bottom <= window.innerHeight + 1);
    await layout(page, dialog, 'landscape');
    await dialog.getByRole('button', { name: '关闭设置' }).click();
    await trigger.waitFor();
    await page.getByRole('button', { name: '切换 AoW 节点' }).click();
    await page.getByRole('menuitem', { name: /phone-node/ }).waitFor();
    await page.keyboard.press('Escape');
    await trigger.click();
    await choose('通知');
    assert.equal(await dialog.getByRole('checkbox', { name: '微信推送' }).isChecked(), true);
  });

  await test('mobile drafts survive category back, failed saves, and cancelled close; saving blocks navigation', async t => {
    const { page, state, dialog, choose, save } = await fixture(t);
    await choose('Nodes');
    const addresses = dialog.getByRole('textbox', { name: '节点地址', exact: true });
    await addresses.fill('invalid-address');
    await save();
    await dialog.getByRole('alert').waitFor();
    assert.equal(state.writes.length, 0);
    await addresses.fill('https://draft.example.com');
    await choose('Editor');
    await choose('Nodes');
    assert.equal(await addresses.inputValue(), 'https://draft.example.com');
    state.failSave = true;
    await save();
    await dialog.getByRole('alert').filter({ hasText: '配置写入失败' }).waitFor();
    assert.deepEqual(state.settings.node_addresses, []);
    page.once('dialog', prompt => prompt.dismiss());
    await dialog.getByRole('button', { name: '关闭设置' }).click();
    assert.equal(await addresses.inputValue(), 'https://draft.example.com');
    state.failSave = false;
    let release;
    state.holdSave = new Promise(resolve => { release = resolve; });
    try {
      await save();
      assert.equal(await dialog.getByRole('button', { name: '返回设置分类' }).isDisabled(), true);
      assert.equal(await dialog.getByRole('button', { name: '关闭设置' }).isDisabled(), true);
    } finally { release(); state.holdSave = undefined; }
    await dialog.getByRole('status').filter({ hasText: '节点地址已保存' }).waitFor();

    await choose('通知');
    await dialog.getByRole('checkbox', { name: '页面提示' }).uncheck();
    assert.equal(await dialog.getByRole('button', { name: '保存', exact: true }).isDisabled(), true);
    await dialog.getByRole('checkbox', { name: '微信推送' }).check();
    await dialog.getByRole('button', { name: '返回设置分类' }).click();
    await choose('通知');
    assert.equal(await dialog.getByRole('checkbox', { name: '微信推送' }).isChecked(), true);
    await dialog.getByRole('button', { name: '返回设置分类' }).click();
    page.once('dialog', prompt => prompt.dismiss());
    await choose('IM');
    await dialog.getByRole('navigation', { name: '设置分类' }).waitFor();
    page.once('dialog', prompt => prompt.accept());
    await choose('IM');
    await dialog.getByRole('textbox', { name: '飞书 App ID' }).fill('unsaved');
    page.once('dialog', prompt => prompt.dismiss());
    await dialog.getByRole('button', { name: '关闭设置' }).click();
    assert.equal(await dialog.getByRole('textbox', { name: '飞书 App ID' }).inputValue(), 'unsaved');
    page.once('dialog', prompt => prompt.accept());
    await dialog.getByRole('button', { name: '关闭设置' }).click();
    await page.getByRole('heading', { name: '你的项目' }).waitFor();
  });

  await test('desktop shares categories and keeps editor, configuration, and provider save behavior', async t => {
    const { page, state, dialog, choose, save } = await fixture(t, 1280, false);
    assert.deepEqual(await dialog.locator('nav strong').allTextContents(), categories);
    await choose('Editor');
    await dialog.getByRole('checkbox', { name: /Word Wrap/ }).check();
    await save('已保存 Editor 配置');
    assert.equal(state.settings.editor.word_wrap, true);
    await choose('Configuration');
    await dialog.getByRole('radio', { name: versions[1], exact: true }).check();
    await choose('Pull Requests');
    await dialog.getByRole('textbox', { name: 'Provider 显示名称' }).fill('Desktop GitHub');
    await save('Provider 配置和脚本已保存');
    await choose('Configuration');
    assert.equal(await dialog.getByRole('radio', { name: versions[1], exact: true }).isChecked(), true);
    page.once('dialog', prompt => prompt.dismiss());
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    assert.equal(await dialog.isVisible(), true);
    await save('重启服务后生效');
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
  });

  await test('mobile forms and save actions follow the visible keyboard viewport', async t => {
    const { page, dialog, choose, save } = await fixture(t, 390, true, true);
    await choose('通知');
    const address = dialog.getByRole('textbox', { name: 'AoW 访问地址' });
    await address.fill('https://keyboard.example.com');
    await page.evaluate(() => {
      Object.assign(window.visualViewport, { height: 420, offsetTop: 60 });
      window.visualViewport.dispatchEvent(new Event('resize'));
    });
    await page.waitForFunction(() => document.querySelector('.mobile-settings').getBoundingClientRect().bottom === 480);
    const footer = await dialog.locator('.project-aow-dialog-footer:visible').boundingBox();
    assert.ok(footer.y >= 60 && footer.y + footer.height <= 480);
    assert.ok(await address.evaluate(element => parseFloat(getComputedStyle(element).fontSize) >= 16), 'inputs avoid iOS focus zoom');
    await save('通知配置已保存');
    await page.evaluate(() => {
      Object.assign(window.visualViewport, { height: 844, offsetTop: 0 });
      window.visualViewport.dispatchEvent(new Event('resize'));
    });
    await page.waitForFunction(() => document.querySelector('.mobile-settings').getBoundingClientRect().bottom === 844);
    await layout(page, dialog, 'keyboard-dismissed');
  });
} finally { await browser.close(); await server.close(); }
