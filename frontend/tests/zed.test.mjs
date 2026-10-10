import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { parse } from 'jsonc-parser';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  async function fixture(t, settings = false, initial = {}) {
    const page = await browser.newPage();
    page.setDefaultTimeout(8000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    const session = { id: 'local', remote_id: 'remote', agent_id: 'test', cwd: '/repo', title: 'Conversation', status: 'idle', revision: 1, updated_at: '', entries: [], permissions: [], modes: {}, config_options: [], commands: [], usage: null };
    const state = { session, created: false, actions: [], saved: '', connectionGate: undefined, connectionError: '', connectionStatus: null, installGate: undefined, installError: '', installStatus: null, installed: false, installCalls: 0, connections: 0, listCalls: [], imports: [], loads: 0, deletes: 0, authRequired: false, authenticated: false, sessions: [], content: '// keep comment\n{ "agent_servers": {}, "future": { "keep": true }, }\n' };
    Object.assign(state, initial);
    const origin = initial.insecure ? `http://acp.test:${server.httpServer.address().port}` : base;
    if (initial.insecure) await page.route(`${origin}/**`, async route => {
      const url = new URL(route.request().url());
      const response = await route.fetch({ url: `${base}${url.pathname}${url.search}`, headers: { ...route.request().headers(), host: new URL(base).host } });
      await route.fulfill({ response });
    });
    await page.route('**/api/zed/**', async route => {
      const request = route.request(); const url = new URL(request.url()); const path = url.pathname; let data = [];
      if (path.endsWith('/agents')) data = state.agents || [{ id: 'test', name: 'Test Agent', supported: true, configured: true, installed: true }, ...(state.saved ? [{ id: 'codex-acp', name: 'Codex ACP', supported: true, configured: true, installed: state.installed }] : [])];
      else if (path.endsWith('/registry')) data = [{ id: 'codex-acp', name: 'Codex ACP', description: 'Registry agent', supported: true }];
      else if (path.endsWith('/settings')) {
        if (request.method() === 'PUT') { state.saved = request.postDataJSON().content; state.content = state.saved; }
        data = { path: '/config/zed/settings.json', revision: 'v1', content: state.content };
      } else if (path.endsWith('/installation')) {
        if (request.method() === 'POST') {
          state.installCalls++;
          assert.equal(parse(state.content).agent_servers['codex-acp'].type, 'registry');
          if (state.installGate) await state.installGate;
          if (state.installError) { await route.fulfill({ status: 400, json: { message: state.installError } }); return; }
          state.installed = true;
        }
        data = state.installStatus;
      } else if (path === '/api/zed/connection-status') data = state.connectionStatus;
      else if (path === '/api/zed/connections') {
        state.connections++;
        if (state.connectionGate) await state.connectionGate;
        if (state.connectionError) { await route.fulfill({ status: 400, json: { message: state.connectionError } }); return; }
        data = { id: 'connection', agent_id: 'test', cwd: '/repo', auth_methods: state.authRequired || state.advertiseAuth ? [{ id: 'login', name: '登录 Agent', type: 'agent' }] : [], capabilities: state.capabilities || { loadSession: true, sessionCapabilities: { list: {} } } };
      }
      else if (path === '/api/zed/connections/connection/authenticate') { state.authenticating = true; if (state.authGate) await state.authGate; state.authenticated = true; state.authenticating = false; for (const item of state.sessions) { item.auth_required = false; item.error = null; item.revision++; } data = {}; }
      else if (path === '/api/zed/connections/connection/requests') {
        if (request.method() === 'POST') { state.authAnswer = request.postDataJSON(); state.finishAuth?.(); data = {}; }
        else data = state.authenticating ? state.authRequests || [] : [];
      }
      else if (path === '/api/zed/connections/connection/sessions' && request.method() === 'GET') {
        state.listCalls.push(url.searchParams.get('cursor'));
        if (state.historyError) { await route.fulfill({ status: 400, json: { message: state.historyError } }); return; }
        if (state.historyAuthRequired && !state.authenticated) { await route.fulfill({ status: 400, json: { code: 'acp_auth_required', message: 'Sign in to read history' } }); return; }
        data = state.pages?.[url.searchParams.get('cursor') || 'first'] || { sessions: state.history || [{ sessionId: 'remote-old', title: 'Earlier conversation', cwd: '/repo' }] };
      }
      else if (path === '/api/zed/connections/connection/import') {
        const selected = request.postDataJSON(); state.imports.push(selected);
        if (state.importError) { await route.fulfill({ status: 400, json: { message: state.importError } }); return; }
        data = [];
        for (const item of selected) {
          if (state.sessions.some(saved => saved.agent_id === 'test' && saved.cwd === item.cwd && saved.remote_id === item.remote_id)) continue;
          const imported = { ...session, ...item, id: `imported-${state.sessions.length}`, agent_id: 'test', title: item.title || 'New conversation', entries: [], status: 'disconnected', needs_load: true };
          state.sessions.push(imported); data.push(imported);
        }
      }
      else if (path === '/api/zed/connections/connection/load') {
        data = state.sessions.find(item => item.remote_id === request.postDataJSON().remote_id);
        if (!data) { data = { ...session, id: 'history', remote_id: 'remote-old', title: 'Earlier conversation' }; state.sessions.push(data); }
      }
      else if (path === '/api/zed/connections/connection/sessions') {
        if (state.authRequired && !state.authenticated) { await route.fulfill({ status: 400, json: { code: 'acp_auth_required', message: 'Authentication required' } }); return; }
        if (state.sessionError) { await route.fulfill({ status: 400, json: { code: 'acp_error', message: state.sessionError } }); return; }
        state.created = true;
        data = state.sessions.length ? { ...session, id: `local-${state.sessions.length + 1}`, title: `Conversation ${state.sessions.length + 1}` } : session;
        state.sessions.push(data);
      }
      else if (path === '/api/zed/sessions') data = state.sessions.filter(item => item.cwd === url.searchParams.get('cwd'));
      else if (/^\/api\/zed\/sessions\/[^/]+$/.test(path)) {
        if (request.method() === 'DELETE') { state.deletes++; state.sessions = state.sessions.filter(item => item.id !== path.split('/').at(-1)); }
        else { state.snapshots = (state.snapshots || 0) + 1; data = state.sessions.find(item => item.id === path.split('/').at(-1)); }
      }
      else if (path.endsWith('/actions')) {
        const session = state.sessions.find(item => item.id === path.split('/').at(-2));
        const action = request.postDataJSON(); state.actions.push(action);
        if (action.action === 'resume' && state.resumeAuthRequired && !state.authenticated) { await route.fulfill({ status: 400, json: { code: 'acp_auth_required', message: 'Sign in to restore this session' } }); return; }
        if (action.action === 'resume' && state.resumeGate) await state.resumeGate;
        if (action.action === 'resume' && state.resumeError) { await route.fulfill({ status: 400, json: { message: state.resumeError } }); return; }
        if (action.action === 'prompt' && state.promptGate) await state.promptGate;
        session.revision++;
        if (action.action === 'prompt') {
          session.entries = [{ id: 'user', kind: 'user', content: action.content[0] }, { id: 'assistant', kind: 'assistant', content: { type: 'text', text: 'Hello from ACP' } }, { id: 'tool', kind: 'tool', content: { title: 'Read source', status: 'in_progress', locations: [{ path: '/repo/main.rs', line: 12 }], content: [{ type: 'diff', path: '/repo/main.rs', oldText: 'old', newText: 'new' }] } }];
          session.permissions = [{ id: 'permission', kind: 'permission', request: { toolCall: { title: 'Write source' }, options: [{ optionId: 'yes', name: 'Allow once' }] } }];
        } else if (action.action === 'answer') session.permissions = [];
        else if (action.action === 'dismiss_notice') session.notices = session.notices.filter(notice => notice.id !== action.notice_id);
        else if (action.action === 'set_config') session.config_options = session.config_options.map(option => (option.configId || option.id) === action.config_id ? { ...option, currentValue: action.value } : option);
        else if (action.action === 'set_mode') session.modes.currentModeId = action.mode_id;
        else if (action.action === 'resume') { session.status = 'idle'; if (session.needs_load) { session.needs_load = false; state.loads++; session.entries = [{ id: 'loaded', kind: 'assistant', content: { type: 'text', text: 'Imported conversation content' } }]; } }
        data = session;
      }
      await route.fulfill({ json: data });
    });
    await page.goto(`${origin}/tests/zed-preview.html${settings ? '?settings' : ''}`);
    if (!settings) await page.waitForFunction(() => document.querySelector('[aria-label="刷新 ACP"]')?.disabled === false);
    return { page, state };
  }
  const newSession = async page => { await page.getByLabel('新建 ACP 会话', { exact: true }).click(); await page.getByRole('menuitem', { name: 'Test Agent', exact: true }).click(); };
  const importHistory = async page => { await page.getByRole('button', { name: '导入 ACP 会话', exact: true }).click(); const dialog = page.getByRole('dialog', { name: '导入外部 Agent 会话' }); await dialog.getByRole('button', { name: '导入 1 个会话', exact: true }).click(); await dialog.waitFor({ state: 'detached' }); };
  await test('new ACP tabs work on LAN HTTP without secure-context APIs', async t => {
    const { page, state } = await fixture(t, false, { insecure: true });
    assert.deepEqual(await page.evaluate(() => [isSecureContext, typeof crypto.randomUUID, typeof crypto.getRandomValues]), [false, 'undefined', 'function']);
    await newSession(page);
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).waitFor();
    await newSession(page);
    await page.getByRole('tab').filter({ hasText: 'Conversation 2' }).waitFor();
    assert.equal(await page.getByRole('tab').count(), 2);
    assert.equal(state.sessions.length, 2);
  });
  await test('resume request authentication survives unchanged snapshots and pending login', async t => {
    const { page, state } = await fixture(t, false, { advertiseAuth: true, resumeAuthRequired: true });
    await newSession(page);
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    const input = content.getByLabel('ACP 消息', { exact: true });
    await input.fill('Keep my recovery draft');
    Object.assign(state.session, { status: 'disconnected', auth_required: false, revision: state.session.revision + 1 });
    await content.getByRole('button', { name: '恢复会话', exact: true }).click();
    const login = content.getByRole('region', { name: 'Agent 登录', exact: true });
    await login.waitFor();
    const pollAgain = async () => {
      for (let index = 0; index < 3; index++) await page.waitForResponse(response => response.url().endsWith('/api/zed/sessions/local') && response.request().method() === 'GET');
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
      assert.equal(await login.isVisible(), true);
    };
    await pollAgain();
    state.session.title = 'Renamed while disconnected'; state.session.revision++;
    await pollAgain();
    state.authGate = new Promise(resolve => { state.finishAuth = resolve; });
    t.after(() => state.finishAuth());
    await login.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await pollAgain();
    state.finishAuth();
    await login.waitFor({ state: 'detached' });
    assert.equal(await input.inputValue(), 'Keep my recovery draft');
    assert.deepEqual(state.actions.map(action => action.action), ['resume']);
    await content.getByRole('button', { name: '恢复会话', exact: true }).click();
    await page.waitForFunction(() => !document.querySelector('[aria-label="ACP 消息"]')?.disabled);
    assert.equal(state.sessions.length, 1);
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'resume']);
    // A successful login in the import dialog must also release this request-only auth state.
    state.authenticated = false; state.historyAuthRequired = true;
    state.session.status = 'disconnected'; state.session.revision++;
    await content.getByRole('button', { name: '恢复会话', exact: true }).click();
    await login.waitFor();
    const sidebar = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    await sidebar.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    await page.getByRole('dialog').getByRole('button', { name: '登录 Agent', exact: true }).click();
    await login.waitFor({ state: 'detached' });
    await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(await input.inputValue(), 'Keep my recovery draft');
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'resume', 'resume']);
  });
  await test('ACP drafts survive portal moves and title updates without crossing tabs', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('First tab draft');
    state.session.title = 'Renamed conversation'; state.session.revision++;
    await page.getByRole('tab').filter({ hasText: 'Renamed conversation' }).waitFor();
    await newSession(page);
    await page.getByRole('tab').filter({ hasText: 'Conversation 2' }).waitFor();
    await input.fill('Second tab draft');
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await input.waitFor();
    assert.equal(await input.inputValue(), 'Second tab draft');
    await page.getByRole('tab').filter({ hasText: 'Renamed conversation' }).click();
    assert.equal(await input.inputValue(), 'First tab draft');
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await input.waitFor();
    assert.equal(await input.inputValue(), 'First tab draft');
    assert.equal(state.sessions.length, 2);
    assert.deepEqual(state.actions, []);
  });
  await test('accepted prompts clear moved drafts without overwriting newer input', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    let finish;
    t.after(() => finish?.());
    for (const newer of ['', 'Continue editing']) {
      state.promptGate = new Promise(resolve => { finish = resolve; });
      await input.fill('Send this draft');
      const submitted = page.waitForRequest(request => request.url().endsWith('/actions') && request.postDataJSON().action === 'prompt');
      await page.getByRole('button', { name: '发送', exact: true }).click();
      await submitted;
      await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
      await input.waitFor();
      assert.equal(await input.inputValue(), 'Send this draft');
      if (newer) await input.fill(newer);
      const accepted = page.waitForResponse(response => response.url().endsWith('/actions'));
      finish(); await accepted;
      await page.waitForFunction(value => document.querySelector('[aria-label="ACP 消息"]')?.value === value, newer);
    }
    assert.equal(state.actions.filter(action => action.action === 'prompt').length, 2);
  });
  await test('closing an ACP tab discards its view draft without deleting the session', async t => {
    const { page, state } = await fixture(t);
    await importHistory(page);
    const history = page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true });
    await history.click();
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('View-only draft');
    await page.getByRole('button', { name: '关闭 Earlier conversation', exact: true }).click();
    await history.click();
    await input.waitFor();
    assert.equal(await input.inputValue(), '');
    assert.equal(state.sessions.length, 1);
    assert.equal(state.deletes, 0);
  });
  await test('ACP conversation renders tool locations and answers explicit permissions', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).fill('hello');
    await page.getByRole('button', { name: '发送', exact: true }).click();
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    const tool = page.getByRole('region', { name: 'Read source', exact: true });
    await tool.getByRole('button', { name: 'Read source', exact: true }).click();
    await tool.locator('.monaco-diff-editor').waitFor();
    await page.waitForFunction(() => {
      const diff = document.querySelector('.zed-diff .monaco-diff-editor');
      return diff?.textContent.includes('old') && diff.textContent.includes('new');
    });
    await page.getByRole('button', { name: '/repo/main.rs:12', exact: true }).click();
    assert.equal(await page.getByLabel('Opened file').textContent(), '/repo/main.rs:12');
    await page.getByRole('button', { name: 'Allow once', exact: true }).click();
    await page.getByRole('region', { name: 'Agent 权限请求' }).waitFor({ state: 'detached' });
    assert.deepEqual(state.actions[0], { action: 'prompt', content: [{ type: 'text', text: 'hello' }] });
    assert.deepEqual(state.actions[1], { action: 'answer', request_id: 'permission', response: { outcome: { outcome: 'selected', optionId: 'yes' } } });
  });
  await test('Registry additions retain comments and unknown JSONC fields', async t => {
    const { page, state } = await fixture(t, true);
    await page.getByRole('button', { name: '浏览 ACP Registry', exact: true }).click();
    await page.getByRole('button', { name: '添加并安装', exact: true }).click();
    await page.getByText('codex-acp 已安装，可在 ACP 面板中新建会话。').waitFor();
    assert.equal(state.installCalls, 1);
    assert.equal(state.connections, 0);
    assert.equal(state.created, false);
    assert.ok(state.saved.includes('// keep comment'));
    assert.deepEqual(parse(state.saved).future, { keep: true });
    assert.equal(parse(state.saved).agent_servers['codex-acp'].type, 'registry');
    assert.equal(await page.getByLabel('Unsaved settings').textContent(), 'false');
  });
  await test('Registry installation reports progress and allows retry without creating a session', async t => {
    const { page, state } = await fixture(t, true);
    let finish;
    state.installGate = new Promise(resolve => { finish = resolve; });
    t.after(() => finish());
    state.installStatus = { phase: 'installing', detail: null, running: true, elapsed_seconds: 18 };
    await page.getByRole('button', { name: '浏览 ACP Registry', exact: true }).click();
    await page.getByRole('button', { name: '添加并安装', exact: true }).click();
    await page.getByText('正在下载并安装 Agent 依赖… · 已等待 18 秒', { exact: true }).waitFor();
    state.installStatus = { ...state.installStatus, detail: 'ETIMEDOUT', elapsed_seconds: 20 };
    await page.getByText(/网络连接失败（ETIMEDOUT）/).waitFor();
    assert.equal(state.created, false);
    assert.equal(state.connections, 0);
    state.installError = 'npm 下载失败，请检查服务代理';
    finish();
    await page.getByRole('alert').getByText(state.installError, { exact: true }).waitFor();
    state.installError = '';
    await page.getByRole('button', { name: '重试安装', exact: true }).first().click();
    await page.getByText('codex-acp 已安装，可在 ACP 面板中新建会话。').waitFor();
    assert.equal(state.installCalls, 2);
    assert.equal(state.connections, 0);
  });
  await test('ACP sessions use central tabs, deduplicate opens and survive closing or refreshing a view', async t => {
    const { page, state } = await fixture(t);
    const sidebar = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    assert.equal(await sidebar.getByLabel('ACP 消息', { exact: true }).count(), 0);
    await newSession(page);
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).waitFor();
    await sidebar.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    assert.equal(await page.getByRole('tab').count(), 1);
    await page.getByRole('button', { name: '关闭 Conversation', exact: true }).click();
    assert.equal(await page.getByRole('tab').count(), 0);
    assert.equal(state.deletes, 0);
    assert.equal(state.sessions.length, 1);
    await sidebar.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(state.connections, 1);
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).count(), 0);
    assert.equal(state.connections, 1, 'restoring local history does not connect to agents');
    assert.equal(state.sessions.length, 1);
    await newSession(page);
    await page.getByRole('tab').filter({ hasText: 'Conversation 2' }).waitFor();
    assert.equal(await page.getByRole('tab').count(), 2);
    assert.equal(state.sessions.length, 2);
  });
  await test('new ACP tabs open immediately and recover from connection failures in the same tab', async t => {
    const { page, state } = await fixture(t);
    let finish;
    state.connectionGate = new Promise(resolve => { finish = resolve; });
    t.after(() => finish());
    state.connectionStatus = { phase: 'initializing', running: true, elapsed_seconds: 2 };
    await newSession(page);
    await page.getByRole('tab').filter({ hasText: '新会话' }).waitFor();
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByText(/正在建立 ACP 连接/).waitFor();
    assert.equal(state.created, false);
    state.connectionError = 'Adapter connection failed';
    finish();
    await content.getByRole('alert').getByText('Adapter connection failed', { exact: true }).waitFor();
    state.connectionError = '';
    await content.getByRole('button', { name: '重试打开会话', exact: true }).click();
    await content.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await page.getByRole('tab').count(), 1);
    assert.equal(state.connections, 2);
    assert.equal(state.sessions.length, 1);
  });
  await test('authentication required for a new session remains interactive inside its tab', async t => {
    const { page, state } = await fixture(t);
    state.authRequired = true;
    await newSession(page);
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByRole('alert').getByText('Authentication required', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 0);
    await content.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await content.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await content.getByRole('region', { name: 'Agent 登录', exact: true }).count(), 0);
    assert.equal(state.authenticated, true);
    assert.equal(state.sessions.length, 1);
    assert.equal(await page.getByRole('tab').count(), 1);
  });
  await test('moving a pending ACP tab between portal hosts creates only one session', async t => {
    const { page, state } = await fixture(t);
    let finish;
    state.connectionGate = new Promise(resolve => { finish = resolve; });
    t.after(() => finish());
    await newSession(page);
    await page.waitForResponse(response => response.url().includes('/api/zed/connection-status'));
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await page.getByRole('region', { name: 'ACP 会话内容', exact: true }).waitFor();
    finish();
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(state.connections, 1);
    assert.equal(state.sessions.length, 1);
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 1);
  });
  await test('Agent history opens a separate tab and reuses it when opened again', async t => {
    const { page, state } = await fixture(t);
    await importHistory(page);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    const openHistory = async () => {
      await page.getByRole('region', { name: 'ACP 会话列表', exact: true }).getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).click();
      await page.getByRole('tab', { name: /^Earlier conversation/ }).waitFor();
    };
    await openHistory();
    assert.equal(await page.getByRole('tab').count(), 2);
    await page.getByRole('tab', { name: /^Conversation/ }).click();
    await openHistory();
    assert.equal(await page.getByRole('tab').count(), 2);
    assert.equal(state.sessions.length, 2);
  });
  await test('ACP forms preserve typed defaults and validate titled multi-selects', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    state.session.permissions = [{ id: 'form', kind: 'elicitation', request: { mode: 'form', message: 'Configure access', requestedSchema: { type: 'object', required: ['model', 'enabled', 'count', 'scopes'], properties: {
      model: { type: 'string', title: 'Model', oneOf: [{ const: 'small', title: 'Small' }, { const: 'large', title: 'Large' }], default: 'large' },
      enabled: { type: 'boolean', title: 'Enabled', default: false },
      count: { type: 'integer', title: 'Count', minimum: 1, maximum: 5, default: 2 },
      scopes: { type: 'array', title: 'Scopes', minItems: 2, maxItems: 2, items: { anyOf: [{ const: 'read', title: 'Read' }, { const: 'write', title: 'Write' }] } },
    } } } }];
    state.session.revision++;
    const form = page.getByRole('form', { name: 'Agent 信息请求' });
    await form.waitFor();
    assert.equal(await form.getByLabel('Model', { exact: true }).inputValue(), 'large');
    await form.getByLabel('Scopes', { exact: true }).selectOption(['read']);
    await form.getByRole('button', { name: '确认', exact: true }).click();
    await form.getByRole('alert').getByText('Scopes：所选数量不符合要求。', { exact: true }).waitFor();
    assert.equal(state.actions.length, 0);
    await form.getByLabel('Scopes', { exact: true }).selectOption(['read', 'write']);
    await form.getByRole('button', { name: '确认', exact: true }).click();
    await form.waitFor({ state: 'detached' });
    assert.deepEqual(state.actions[0].response, { action: 'accept', content: { model: 'large', enabled: false, count: 2, scopes: ['read', 'write'] } });
  });
  await test('the host imports only the ACP facade and places ACP after Terminal', async () => {
    const source = await readFile(new URL('../src/aow/ProjectAow.tsx', import.meta.url), 'utf8');
    assert.ok(source.includes("from '../features/zed'"));
    assert.ok(!source.includes("from '../features/zed/"));
    const navigation = source.slice(source.indexOf('<nav aria-label="AoW side views">'));
    assert.ok(navigation.indexOf('aria-label="Terminal 面板"') < navigation.indexOf('aria-label="ACP 面板"'));
    assert.ok(navigation.indexOf('aria-label="ACP 面板"') < navigation.indexOf('aria-label="Conversation"'));
  });
  await test('Zed config providers replace legacy modes and selecting a value persists its default', async t => {
    const { page, state } = await fixture(t);
    state.advertiseAuth = true;
    Object.assign(state.session, {
      modes: { currentModeId: 'legacy', availableModes: [{ id: 'legacy', name: 'Legacy mode' }] }, config_options_supported: true,
      config_options: [
        { configId: 'mode', name: 'Mode', type: 'select', currentValue: 'auto', options: [{ value: 'auto', name: 'Auto review' }, { value: 'ask', name: 'Ask' }] },
        { configId: 'collaboration', name: 'Collaboration mode', type: 'select', currentValue: 'default', options: [{ value: 'default', name: 'Default' }] },
        { configId: 'model', name: 'Model', type: 'select', currentValue: 'astra', options: [{ groupId: 'openai', name: 'OpenAI', options: [{ value: 'astra', name: '6 Astra' }, { value: 'sol', name: '6 Sol' }, { value: 'mini', name: 'Mini' }, { value: 'fast', name: 'Fast' }, { value: 'large', name: 'Large' }] }] },
        { configId: 'effort', name: 'Reasoning effort', type: 'select', currentValue: 'xhigh', options: [{ value: 'xhigh', name: 'Xhigh' }] },
        { configId: 'fast', name: 'Fast mode', type: 'boolean', currentValue: false },
        { configId: 'future', name: 'Unknown control', type: '_future', currentValue: 'x' },
      ],
      usage: { used: 3200, size: 128000 },
      entries: [{ id: 'u', kind: 'user', content: { type: 'text', text: 'hi' } }, { id: 'a', kind: 'assistant', content: { type: 'text', text: 'Hi! What can I help you with?' } }],
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await page.getByRole('region', { name: 'Agent 登录', exact: true }).count(), 0);
    assert.equal(await page.getByRole('button', { name: '模式', exact: true }).count(), 0);
    assert.equal(await page.getByText('Unknown control', { exact: true }).count(), 0);
    assert.equal(await page.getByRole('button', { name: '读取 Agent 历史', exact: true }).count(), 0);
    const composer = await page.locator('.zed-composer').boundingBox();
    const options = await page.getByLabel('会话配置', { exact: true }).boundingBox();
    assert.ok(options.y > composer.y && options.y + options.height <= composer.y + composer.height);
    await page.getByRole('button', { name: '移动会话视图', exact: true }).evaluate(button => { button.style.display = 'none'; });
    await page.screenshot({ path: '/tmp/aow-zed-aligned-conversation.png' });
    await page.getByRole('button', { name: 'Model', exact: true }).click();
    await page.getByRole('menu', { name: 'Model', exact: true }).getByText('OpenAI', { exact: true }).waitFor();
    await page.getByLabel('搜索 Model', { exact: true }).fill('sol');
    await page.getByRole('button', { name: '收藏 6 Sol', exact: true }).click();
    await page.getByRole('menuitemradio', { name: '6 Sol', exact: true }).first().click();
    await page.getByRole('button', { name: 'Model', exact: true }).getByText('6 Sol', { exact: true }).waitFor();
    assert.equal(parse(state.saved).agent_servers.test.default_config_options.model, 'sol');
    assert.deepEqual(parse(state.saved).agent_servers.test.favorite_config_option_values.model, ['sol']);
    assert.ok(state.saved.includes('// keep comment'));
    await page.getByRole('switch', { name: 'Fast mode', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('[role="switch"]')?.getAttribute('aria-checked') === 'true');
    assert.equal(parse(state.saved).agent_servers.test.default_config_options.fast, true);
    assert.equal(state.actions.at(-1).value, true);
  });
  await test('empty config providers suppress legacy controls while absent providers fall back to modes', async t => {
    const { page, state } = await fixture(t);
    state.session.config_options_supported = true;
    state.session.modes = { currentModeId: 'code', availableModes: [{ id: 'code', name: 'Code' }] };
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '模式', exact: true }).count(), 0);
    state.session.config_options_supported = false; state.session.revision++;
    await page.getByRole('button', { name: '模式', exact: true }).getByText('Code', { exact: true }).waitFor();
  });
  await test('rendering follows Zed visibility, streamed thinking and tool permission rules', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: '**literal user input**' } },
      { id: 'empty', kind: 'assistant', content: { type: 'text', text: '   ' } },
      { id: 'thought', kind: 'thought', content: { type: 'text', text: 'Consider the request carefully.' } },
    ] });
    await newSession(page);
    const thought = page.getByRole('button', { name: '思考过程', exact: true });
    await page.waitForFunction(() => document.querySelector('[aria-label="思考过程"]')?.getAttribute('aria-expanded') === 'true');
    assert.equal(await page.locator('.zed-user-text').innerText(), '**literal user input**');
    state.session.entries.push({ id: 'answer', kind: 'assistant', content: { type: 'text', text: '**Answer**' } }); state.session.revision++;
    await page.getByText('Answer', { exact: true }).waitFor();
    assert.equal(await thought.getAttribute('aria-expanded'), 'false');
    await thought.click();
    state.session.entries.push(
      { id: 'tool', kind: 'tool', content: { toolCallId: 'tool', title: 'Read source', kind: 'read', status: 'in_progress', content: [{ type: 'content', content: { type: 'text', text: 'Tool result' } }] } },
      { id: 'cancelled', kind: 'tool', content: { title: 'Cancelled without output', status: 'cancelled', content: [] } },
      { id: 'future', kind: 'extension', content: { secret: 'Wire metadata is not a message' } },
    ); state.session.revision++;
    const tool = page.getByRole('region', { name: 'Read source', exact: true });
    await tool.waitFor();
    assert.equal(await tool.getByRole('button', { name: 'Read source', exact: true }).getAttribute('aria-expanded'), 'false');
    assert.equal(await page.getByText('Cancelled without output', { exact: true }).count(), 0);
    assert.equal(await page.getByText('Wire metadata is not a message', { exact: true }).count(), 0);
    assert.equal(await thought.getAttribute('aria-expanded'), 'true', 'manual expansion survives streamed updates');
    state.session.permissions = [{ id: 'p', kind: 'permission', request: { toolCall: { toolCallId: 'tool', title: 'Read source' }, options: [{ optionId: 'yes', name: 'Allow once' }] } }]; state.session.revision++;
    await tool.getByText('Tool result', { exact: true }).waitFor();
    await tool.getByRole('button', { name: 'Allow once', exact: true }).click();
    assert.equal(state.actions.at(-1).request_id, 'p');
  });
  await test('reply controls appear only at completed turn ends and jump to the user prompt', async t => {
    const { page, state } = await fixture(t);
    const entry = (id, kind, value) => ({ id, kind, content: { type: 'text', text: value } });
    Object.assign(state.session, { status: 'working', entries: [entry('u1', 'user', 'First prompt'), entry('a1', 'assistant', 'First answer'), entry('u2', 'user', 'Second prompt'), entry('a2', 'assistant', 'Checking files')] });
    await newSession(page);
    await page.getByText('Checking files', { exact: true }).waitFor();
    const actions = page.locator('.zed-message-actions');
    assert.equal(await actions.count(), 1, 'only the previous completed turn has controls');
    state.session.entries.push({ id: 'command', kind: 'tool', content: { title: 'Read files', kind: 'execute', status: 'completed', content: [] } }, entry('a3', 'assistant', 'Final answer'));
    state.session.revision++;
    await page.getByText('Final answer', { exact: true }).waitFor();
    assert.equal(await actions.count(), 1, 'streamed final text is still generating');
    state.session.status = 'idle'; state.session.revision++;
    await page.waitForFunction(() => document.querySelectorAll('.zed-message-actions').length === 2);
    assert.equal(await page.locator('[data-entry-id="a2"] .zed-message-actions').count(), 0);
    await page.evaluate(() => {
      navigator.clipboard.writeText = async value => { window.copiedReply = value; };
      Element.prototype.scrollIntoView = function () { window.scrolledEntry = this.getAttribute('data-entry-id'); };
    });
    await actions.last().getByRole('button', { name: '复制回复', exact: true }).click();
    assert.equal(await page.evaluate(() => window.copiedReply), 'Checking files\n\nFinal answer');
    await actions.last().getByRole('button', { name: '跳到用户消息', exact: true }).click();
    assert.equal(await page.evaluate(() => window.scrolledEntry), 'u2', 'skip intervening tool and commentary');
    state.session.permissions = [{ id: 'pending', kind: 'permission', request: { toolCall: { title: 'Confirmation' }, options: [] } }]; state.session.revision++;
    await page.getByRole('region', { name: 'Agent 权限请求', exact: true }).waitFor();
    await page.waitForFunction(() => document.querySelectorAll('.zed-message-actions').length === 1);
    state.session.permissions = []; state.session.status = 'working';
    state.session.entries.push(entry('u3', 'user', 'Third prompt'), entry('a4', 'assistant', 'Next response')); state.session.revision++;
    await page.getByText('Next response', { exact: true }).waitFor();
    assert.equal(await actions.count(), 2, 'historical turn controls remain while the next turn runs');
    state.session.status = 'idle'; state.session.stop_reason = 'cancelled'; state.session.revision++;
    await page.waitForFunction(() => document.querySelectorAll('.zed-message-actions').length === 3);
  });
  await test('tool endings retain turn controls and copy only assistant content', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Inspect files' } },
      { id: 'a', kind: 'assistant', content: { type: 'text', text: 'Checking README' } },
      { id: 'thought', kind: 'thought', content: { type: 'text', text: 'Private reasoning' } },
      { id: 'link', kind: 'assistant', content: { type: 'resource_link', uri: 'file:///repo/README.md' } },
      { id: 'tool', kind: 'tool', content: { title: 'Read files', status: 'completed', rawOutput: 'Tool output' } },
    ] });
    await newSession(page);
    await page.getByText('Checking README', { exact: true }).waitFor();
    assert.equal(await page.locator('.zed-message-actions').count(), 0);
    state.session.status = 'idle'; state.session.revision++;
    const actions = page.locator('.zed-message-actions');
    await actions.waitFor();
    assert.equal(await page.locator('[data-entry-id="tool"] .zed-message-actions').count(), 1);
    await page.evaluate(() => {
      navigator.clipboard.writeText = async value => { window.copiedReply = value; };
      Element.prototype.scrollIntoView = function () { window.scrolledEntry = this.getAttribute('data-entry-id'); };
    });
    await actions.getByRole('button', { name: '复制回复', exact: true }).click();
    assert.equal(await page.evaluate(() => window.copiedReply), 'Checking README\n\nfile:///repo/README.md');
    await actions.getByRole('button', { name: '跳到用户消息', exact: true }).click();
    assert.equal(await page.evaluate(() => window.scrolledEntry), 'u');
    state.session.entries.at(-1).content = { title: 'Read files', status: 'cancelled' };
    state.session.stop_reason = 'cancelled'; state.session.revision++;
    await page.locator('[data-entry-id="tool"]').waitFor({ state: 'detached' });
    assert.equal(await actions.count(), 1, 'hidden canceled tools still leave a completed turn footer');
  });
  await test('permission previews stay expanded and canceled pending tools remain actionable', async t => {
    const { page, state } = await fixture(t);
    const content = { toolCallId: 'edit', title: 'Review file changes', kind: 'edit', status: 'pending', content: [{ type: 'diff', path: '/repo/main.rs', oldText: 'const before = 1;', newText: 'const after = 2;' }] };
    // The adapter upserts the request's toolCall before exposing the permission.
    state.session.entries = [{ id: 'edit', kind: 'tool', content }];
    state.session.permissions = [{ id: 'permission', kind: 'permission', request: { toolCall: content, options: [{ optionId: 'allow', name: 'Allow once', kind: 'allow_once' }] } }];
    await newSession(page);
    const tool = page.getByRole('region', { name: 'Review file changes', exact: true });
    await tool.locator('.zed-diff .modified-in-monaco-diff-editor').waitFor();
    assert.match(await tool.locator('.zed-diff .modified-in-monaco-diff-editor').textContent(), /const\s+after\s*=\s*2/);
    assert.equal(await tool.getByRole('button', { name: 'Review file changes', exact: true }).getAttribute('aria-expanded'), 'true');
    assert.equal(await page.locator('.zed-message-actions').count(), 0);
    state.session.entries[0].content = { ...content, status: 'cancelled', content: [] }; state.session.revision++;
    await tool.locator('.zed-diff').waitFor({ state: 'detached' });
    await tool.getByRole('button', { name: 'Allow once', exact: true }).click();
    assert.equal(state.actions.at(-1).request_id, 'permission');
  });
  await test('out-of-band plans and notices preserve streamed Markdown and thought expansion', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Inspect' } },
      { id: 'thought', kind: 'thought', content: { type: 'text', text: 'Thinking about files' } },
    ] });
    await newSession(page);
    const thinking = page.getByRole('button', { name: '思考过程', exact: true });
    await page.getByText('Thinking about files', { exact: true }).waitFor();
    state.session.plan = { entries: [{ content: 'Read file', status: 'in_progress' }] };
    state.session.notices = [{ id: 'n', kind: 'notice', content: { severity: 'warning', title: 'Rate limit', description: 'Please **wait** before retrying.' } }];
    state.session.revision++;
    const notice = page.getByRole('alert', { name: 'Rate limit', exact: true });
    await notice.getByText('Please **wait** before retrying.', { exact: true }).waitFor();
    assert.equal(await thinking.getAttribute('aria-expanded'), 'true');
    assert.equal(await notice.getAttribute('data-severity'), 'warning');
    state.session.entries.push({ id: 'a', kind: 'assistant', content: { type: 'text', text: '**Hel' } }); state.session.revision++;
    await page.getByText('**Hel', { exact: true }).waitFor();
    state.session.entries.at(-1).content.text += 'lo**';
    state.session.plan.entries[0].status = 'completed'; state.session.revision++;
    await page.locator('.zed-message-chunk strong').filter({ hasText: /^Hello$/ }).waitFor();
    assert.equal(await page.locator('.zed-message-chunk').count(), 1);
    await page.getByText('1 / 1 项计划已完成', { exact: true }).waitFor();
    await notice.getByRole('button', { name: '关闭通知：Rate limit', exact: true }).click();
    await notice.waitFor({ state: 'detached' });
    assert.deepEqual(state.actions.at(-1), { action: 'dismiss_notice', notice_id: 'n' });
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await page.locator('.zed-message-chunk strong').filter({ hasText: /^Hello$/ }).waitFor();
    assert.equal(await notice.count(), 0);
  });
  await test('compaction cards follow lifecycle and expose only available details', async t => {
    const { page, state } = await fixture(t);
    const content = { compactionId: 'c', status: 'in_progress', summary: [] };
    Object.assign(state.session, { status: 'working', entries: [{ id: 'c', kind: 'compaction_update', content }] });
    await newSession(page);
    await page.getByText('上下文压缩中…', { exact: true }).waitFor();
    assert.equal(await page.locator('details.zed-compaction').count(), 0, 'empty state has no expansion');
    content.summary = [{ type: 'text', text: '**Retained summary**' }]; state.session.revision++;
    const card = page.locator('details.zed-compaction');
    await card.locator('summary').click();
    await card.locator('strong').filter({ hasText: 'Retained summary' }).waitFor();
    content.status = 'failed'; content.error = 'Context limit reached'; state.session.revision++;
    await card.getByText('上下文压缩失败', { exact: true }).waitFor();
    await card.getByRole('alert').filter({ hasText: 'Context limit reached' }).waitFor();
    assert.equal(await card.getAttribute('open'), '');
    assert.equal(await page.locator('.zed-compaction').count(), 1);
    content.status = 'completed'; content.error = null;
    content.summary = [{ type: 'text', text: 'Replacement summary' }]; state.session.revision++;
    await card.getByText('Replacement summary', { exact: true }).waitFor();
    assert.equal(await card.getByRole('alert').count(), 0);
    content.status = 'cancelled'; content.summary = []; state.session.revision++;
    await page.getByText('上下文压缩已取消', { exact: true }).waitFor();
    assert.equal(await card.count(), 0);
  });
  await test('Markdown file links resolve through the host without weakening URL sanitization', async t => {
    const { page, state } = await fixture(t);
    state.session.cwd = '/repo/sub';
    state.session.entries = [{ id: 'a', kind: 'assistant', content: { type: 'text', text: '[File](file:///repo/a%20b.rs#L12) [Relative](../README.md:8) [Absolute](/repo/main.rs#L3) [Web](https://example.com) [Unsafe](javascript:alert%281%29) <a data-local-file="%zz" href="#">Malformed</a>' } }];
    await newSession(page);
    const opened = page.getByLabel('Opened file', { exact: true });
    const url = page.url();
    for (const [label, expected] of [['File', '/repo/a b.rs:12'], ['Relative', '/repo/README.md:8'], ['Absolute', '/repo/main.rs:3']]) {
      await page.getByRole('link', { name: label, exact: true }).click();
      assert.equal(await opened.textContent(), expected);
      assert.equal(page.url(), url);
    }
    const web = page.getByRole('link', { name: 'Web', exact: true });
    assert.equal(await web.getAttribute('target'), '_blank');
    assert.equal(await web.getAttribute('rel'), 'noreferrer');
    assert.equal(await page.getByText('Unsafe', { exact: true }).getAttribute('href'), null);
    await page.getByRole('link', { name: 'Malformed', exact: true }).click();
  });
  await test('reply copying supports LAN HTTP and handles denied clipboard operations', async t => {
    const { page, state } = await fixture(t, false, { insecure: true });
    state.session.entries = [{ id: 'a', kind: 'assistant', content: { type: 'text', text: 'LAN reply' } }];
    await newSession(page);
    assert.equal(await page.evaluate(() => typeof navigator.clipboard), 'undefined');
    await page.evaluate(() => {
      const original = document.execCommand.bind(document);
      document.execCommand = (...args) => { window.copiedReply = document.querySelector('.zed-clipboard-input')?.value; return original(...args); };
    });
    const copy = page.getByRole('button', { name: '复制回复', exact: true });
    await copy.click();
    await copy.locator('.lucide-check').waitFor();
    assert.equal(await page.evaluate(() => window.copiedReply), 'LAN reply');
    assert.equal(await page.locator('.zed-clipboard-input').count(), 0);
    await page.evaluate(() => {
      Object.defineProperty(navigator, 'clipboard', { value: { writeText: async () => { throw new Error('denied'); } }, configurable: true });
      document.execCommand = () => false;
    });
    await copy.click();
    await page.getByRole('alert').filter({ hasText: '复制失败，请选择文本后手动复制。' }).waitFor();
    assert.equal(await page.locator('.zed-clipboard-input').count(), 0);
  });
  await test('execute tools display streamed terminals, retained output and raw-output fallback', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', terminals: { term: { output: 'first line\r\n', cwd: '/repo' } }, entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Run commands' } },
      { id: 'command', kind: 'tool', content: { title: 'pwd', kind: 'execute', status: 'in_progress', content: [{ type: 'terminal', terminalId: 'term' }] } },
      { id: 'raw', kind: 'tool', content: { title: 'Raw command', kind: 'execute', status: 'completed', rawOutput: 'raw result', content: [] } },
    ] });
    await newSession(page);
    const tool = page.getByRole('region', { name: 'pwd', exact: true });
    await tool.getByRole('button', { name: 'pwd', exact: true }).click();
    await tool.getByText('first line', { exact: true }).waitFor();
    const screen = await tool.locator('.xterm').elementHandle();
    state.session.terminals.term.output += '\u001b[32msecond line\u001b[0m\r\n'; state.session.revision++;
    await tool.getByText('second line', { exact: true }).waitFor();
    assert.equal(await tool.locator('.xterm').evaluate((element, previous) => element === previous, screen), true);
    state.session.entries[1].content.status = 'completed';
    state.session.terminals.term.exit_status = { exitCode: 0 }; state.session.status = 'idle'; state.session.revision++;
    await tool.getByText('退出码：0', { exact: true }).waitFor();
    await tool.getByRole('button', { name: 'pwd', exact: true }).click();
    await tool.getByRole('button', { name: 'pwd', exact: true }).click();
    await tool.getByText('first line', { exact: true }).waitFor();
    await tool.getByText('second line', { exact: true }).waitFor();
    const raw = page.getByRole('region', { name: 'Raw command', exact: true });
    await raw.getByRole('button', { name: 'Raw command', exact: true }).click();
    await raw.getByText('raw result', { exact: true }).waitFor();
    state.session.entries[2].content.content = [{ type: 'content', content: { type: 'text', text: 'structured result' } }]; state.session.revision++;
    await raw.getByText('structured result', { exact: true }).waitFor();
    assert.equal(await raw.getByText('raw result', { exact: true }).count(), 0, 'structured output takes precedence');
    state.session.entries[2].content.content = []; state.session.entries[2].content.rawOutput = { exitCode: 1, stderr: 'command failed' }; state.session.revision++;
    await raw.getByText('command failed', { exact: false }).waitFor();
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await tool.getByRole('button', { name: 'pwd', exact: true }).click();
    await tool.getByText('first line', { exact: true }).waitFor();
    await tool.getByText('second line', { exact: true }).waitFor();
    await tool.getByText('退出码：0', { exact: true }).waitFor();
  });
  await test('slash commands use advertised suggestions and do not submit until explicitly sent', async t => {
    const { page, state } = await fixture(t);
    state.session.commands = [{ name: 'review', description: 'Review the changes' }, { name: 'help', description: 'Show help' }];
    await newSession(page);
    const input = page.getByLabel('ACP 消息', { exact: true });
    await input.fill('/rev');
    await page.getByRole('option', { name: '/review Review the changes', exact: true }).waitFor();
    await input.press('Enter');
    assert.equal(await input.inputValue(), '/review ');
    assert.equal(state.actions.length, 0);
    await input.press('Enter');
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    assert.equal(state.actions[0].content[0].text, '/review ');
  });
  await test('an authentication-looking generic error does not trigger the login UI', async t => {
    const { page, state } = await fixture(t);
    state.advertiseAuth = true; state.sessionError = 'Authentication required in quoted tool output';
    await newSession(page);
    await page.getByRole('alert').getByText(state.sessionError, { exact: true }).waitFor();
    assert.equal(await page.getByRole('region', { name: 'Agent 登录', exact: true }).count(), 0);
  });
  await test('history authentication handles request-scoped forms without creating a conversation', async t => {
    const { page, state } = await fixture(t);
    state.advertiseAuth = true; state.historyAuthRequired = true;
    state.authGate = new Promise(resolve => { state.finishAuth = resolve; });
    t.after(() => state.finishAuth());
    state.authRequests = [{ id: 'login-form', kind: 'elicitation', request: { mode: 'form', message: 'Login code', requestedSchema: { type: 'object', required: ['code'], properties: { code: { type: 'string', title: 'Code' } } } } }];
    const sidebar = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    await sidebar.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('button', { name: '登录 Agent', exact: true }).click();
    const form = dialog.getByRole('form', { name: 'Agent 信息请求', exact: true });
    await form.getByLabel('Code', { exact: true }).fill('1234');
    await form.getByRole('button', { name: '确认', exact: true }).click();
    await dialog.getByRole('region', { name: 'Agent 登录', exact: true }).waitFor({ state: 'detached' });
    await dialog.getByText('1 个可导入会话', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(await sidebar.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).count(), 0);
    assert.deepEqual(state.authAnswer.response, { action: 'accept', content: { code: '1234' } });
    assert.equal(state.sessions.length, 0);
  });
  await test('existing sessions recover from authentication without losing drafts or replaying prompts', async t => {
    const { page, state } = await fixture(t);
    state.advertiseAuth = true;
    await newSession(page);
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    const input = content.getByLabel('ACP 消息', { exact: true });
    await input.fill('Keep this unsent draft');
    Object.assign(state.session, { auth_required: true, error: '请重新登录', revision: state.session.revision + 1 });
    await content.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await input.waitFor({ state: 'visible' });
    assert.equal(await input.inputValue(), 'Keep this unsent draft');
    assert.equal(state.sessions.length, 1);
    assert.deepEqual(state.actions, []);
    // Authentication completed elsewhere on the shared connection also removes the callout.
    Object.assign(state.session, { auth_required: true, error: '请重新登录', revision: state.session.revision + 1 });
    await content.getByRole('region', { name: 'Agent 登录', exact: true }).waitFor();
    Object.assign(state.session, { auth_required: false, error: null, revision: state.session.revision + 1 });
    await input.waitFor({ state: 'visible' });
    assert.equal(await input.inputValue(), 'Keep this unsent draft');
    assert.deepEqual(state.actions, []);
  });
  await test('history refresh and reload use only local records even when the agent is offline', async t => {
    const { page, state } = await fixture(t);
    assert.equal(state.connections, 0); assert.deepEqual(state.listCalls, []);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    state.connectionError = 'Offline';
    const sidebar = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    await sidebar.getByRole('button', { name: '刷新 ACP', exact: true }).click();
    await sidebar.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).waitFor();
    await page.reload();
    await sidebar.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).waitFor();
    assert.equal(state.connections, 1); assert.deepEqual(state.listCalls, []);
    assert.equal(await sidebar.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).count(), 0);
  });
  await test('explicit import paginates, scopes, deduplicates and persists metadata before lazy loading', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    state.pages = {
      first: { sessions: [{ sessionId: 'remote', title: 'Do not overwrite', cwd: '/repo' }, { sessionId: 'remote-old', title: 'Earlier conversation', cwd: '/repo', updatedAt: '2020-01-01T00:00:00Z' }], nextCursor: 'page2' },
      page2: { sessions: [{ sessionId: 'remote-old', title: 'Earlier conversation', cwd: '/repo' }, { sessionId: 'elsewhere', title: 'Other workspace', cwd: '/other' }, { sessionId: 'missing-cwd', title: 'Unknown workspace' }], nextCursor: 'page2' },
    };
    await page.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByText('1 个可导入会话', { exact: true }).waitFor();
    assert.deepEqual(state.listCalls, [null, 'page2']);
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 0); assert.deepEqual(state.actions, []);
    await dialog.getByLabel('导入 Test Agent', { exact: true }).uncheck();
    assert.equal(await dialog.getByRole('button', { name: '导入 0 个会话', exact: true }).isDisabled(), true);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(state.imports.length, 0);
    await importHistory(page);
    assert.equal(state.sessions.length, 2); assert.equal(state.loads, 0); assert.deepEqual(state.actions, []);
    assert.equal(state.session.title, 'Conversation');
    assert.equal(state.sessions[1].needs_load, true);
    assert.equal(state.sessions[1].updated_at, '2020-01-01T00:00:00Z');
    await page.reload();
    const history = page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true });
    await history.waitFor();
    assert.equal(state.loads, 0); assert.equal(state.listCalls.length, 4);
    await history.click();
    await page.getByText('Imported conversation content', { exact: true }).waitFor();
    assert.equal(state.loads, 1); assert.equal(state.sessions.length, 2);
    await page.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    await dialog.getByText('0 个可导入会话', { exact: true }).waitFor();
    assert.equal(await dialog.getByRole('button', { name: '导入 0 个会话', exact: true }).isDisabled(), true);
  });
  await test('opening imported history survives authentication and portal moves without duplicate loads', async t => {
    const { page, state } = await fixture(t, false, { advertiseAuth: true, resumeAuthRequired: true });
    await importHistory(page);
    await page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).click();
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByRole('region', { name: 'Agent 登录', exact: true }).waitFor();
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 0);
    state.resumeGate = new Promise(resolve => { state.finishResume = resolve; });
    t.after(() => state.finishResume());
    const resuming = page.waitForRequest(request => request.url().endsWith('/actions') && request.postDataJSON().action === 'resume');
    await content.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await resuming;
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await content.getByRole('status').getByText(/正在加载会话…/).waitFor();
    assert.equal(state.actions.length, 2);
    state.finishResume();
    await content.getByText('Imported conversation content', { exact: true }).waitFor();
    assert.equal(state.loads, 1);
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'resume']);
    assert.equal(state.sessions.length, 1);
  });
  await test('failed imported history loading can be retried without losing the registered record', async t => {
    const { page, state } = await fixture(t);
    await importHistory(page);
    state.resumeError = 'Cannot load transcript';
    await page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).click();
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByRole('alert').getByText('Cannot load transcript', { exact: true }).waitFor();
    assert.equal(state.sessions[0].needs_load, true);
    state.resumeError = '';
    await content.getByRole('button', { name: '恢复会话', exact: true }).click();
    await content.getByText('Imported conversation content', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 1);
  });
  await test('an unavailable agent does not prevent importing another agent', async t => {
    const { page, state } = await fixture(t, false, { agents: ['test', 'offline'].map(id => ({ id, name: id === 'test' ? 'Test Agent' : 'Offline Agent', supported: true, installed: true })) });
    await page.route('**/api/zed/connections', async route => {
      if (route.request().postDataJSON().agent_id === 'offline') await route.fulfill({ status: 400, json: { message: 'Agent unavailable' } });
      else await route.fallback();
    });
    await page.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog.getByRole('alert').getByText('Agent unavailable', { exact: true }).waitFor();
    assert.equal(await dialog.getByLabel('导入 Offline Agent', { exact: true }).isDisabled(), true);
    await dialog.getByRole('button', { name: '导入 1 个会话', exact: true }).click();
    await dialog.waitFor({ state: 'detached' });
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 0);
    assert.equal(state.sessions[0].agent_id, 'test');
  });
  await test('ready imports do not wait for a slow agent and late discovery cannot register history', async t => {
    const { page, state } = await fixture(t, false, { agents: ['test', 'slow'].map(id => ({ id, name: id === 'test' ? 'Test Agent' : 'Slow Agent', supported: true, installed: true })) });
    let finish;
    const gate = new Promise(resolve => { finish = resolve; });
    t.after(() => finish());
    await page.route('**/api/zed/connections', async route => {
      if (route.request().postDataJSON().agent_id !== 'slow') { await route.fallback(); return; }
      await gate;
      await route.fulfill({ json: { id: 'connection', agent_id: 'slow', cwd: '/repo', capabilities: { sessionCapabilities: { list: {} } } } });
    });
    await page.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog.locator('label').filter({ hasText: 'Slow Agent' }).getByRole('status').waitFor();
    await dialog.getByRole('button', { name: '导入 1 个会话', exact: true }).click();
    await dialog.waitFor({ state: 'detached' });
    const connected = page.waitForResponse(response => response.url().endsWith('/connections') && response.request().postDataJSON().agent_id === 'slow');
    finish(); await connected;
    await page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).waitFor();
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 0);
    assert.deepEqual(state.listCalls, [null]);
    assert.equal(state.imports.length, 1);
  });
  await test('import handles unsupported listing and retries discovery and persistence failures', async t => {
    const { page, state } = await fixture(t);
    state.capabilities = { loadSession: true };
    const open = page.getByRole('button', { name: '导入 ACP 会话', exact: true });
    await open.click();
    const dialog = page.getByRole('dialog');
    await dialog.getByText('不支持列出会话', { exact: true }).waitFor();
    assert.deepEqual(state.listCalls, []);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    state.capabilities = undefined; state.historyError = 'Cannot list';
    await open.click();
    await dialog.getByRole('alert').getByText('Cannot list', { exact: true }).waitFor();
    state.historyError = '';
    await dialog.getByRole('button', { name: '重试 Test Agent', exact: true }).click();
    state.importError = 'Cannot save';
    await dialog.getByRole('button', { name: '导入 1 个会话', exact: true }).click();
    await dialog.getByRole('alert').getByText('Cannot save', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 0);
    state.importError = '';
    await dialog.getByRole('button', { name: '导入 1 个会话', exact: true }).click();
    await dialog.waitFor({ state: 'detached' });
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 0);
  });
} finally { await browser?.close(); await server.close(); }
