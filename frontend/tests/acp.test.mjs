import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { parse } from 'jsonc-parser';
import { acpControlFixture } from './fixtures/acp-control.mjs';
import { acpEventsFixture, disconnectAcpEvents } from './fixtures/acp-events.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  async function fixture(t, settings = false, initial = {}) {
    const page = await browser.newPage();
    page.setDefaultTimeout(8000);
    // Cold Vite transforms can exceed the interaction deadline on shared CI CPUs.
    page.setDefaultNavigationTimeout(30000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    const session = { thread_id: 'local', session_id: 'remote', agent_id: 'test', cwd: '/repo', title: 'Conversation', status: 'idle', revision: 1, updated_at: '', entries: [], permissions: [], modes: {}, config_options: [], commands: [], usage: null };
    let state = initial.sharedState || { session, created: false, actions: [], saved: '', connectionGate: undefined, connectionError: '', connectionStatus: null, installGate: undefined, installError: '', installStatus: null, installed: false, installCalls: 0, connections: 0, listCalls: [], imports: [], loads: 0, deletes: 0, authRequired: false, authenticated: false, sessions: [], content: '// keep comment\n{ "agent_servers": {}, "future": { "keep": true }, }\n' };
    Object.assign(state, initial);
    state = await acpEventsFixture(page, state);
    state.controls ??= acpControlFixture();
    await state.controls.install(page);
    const origin = initial.insecure ? `http://acp.test:${server.httpServer.address().port}` : base;
    if (initial.insecure) await page.route(`${origin}/**`, async route => {
      const url = new URL(route.request().url());
      const response = await route.fetch({ url: `${base}${url.pathname}${url.search}`, headers: { ...route.request().headers(), host: new URL(base).host } });
      await route.fulfill({ response });
    });
    await page.route('**/api/acp/**', async route => {
      const request = route.request(); const url = new URL(request.url()); const path = url.pathname; let data = [];
      if (path.endsWith('/agents')) {
        if (state.agentsGate) await state.agentsGate;
        if (state.agentsError) { await route.fulfill({ status: 503, json: { message: state.agentsError } }); return; }
        data = state.agents || [{ id: 'test', name: 'Test Agent', supported: true, configured: true, installed: true }, ...(state.saved ? [{ id: 'codex-acp', name: 'Codex ACP', supported: true, configured: true, installed: state.installed }] : [])];
      }
      else if (path.endsWith('/registry')) data = [{ id: 'codex-acp', name: 'Codex ACP', description: 'Registry agent', supported: true }];
      else if (path.endsWith('/settings')) {
        if (request.method() === 'PUT') { state.saved = request.postDataJSON().content; state.content = state.saved; }
        data = { path: '/config/acp/settings.json', revision: 'v1', content: state.content };
      } else if (path.endsWith('/installation')) {
        if (request.method() === 'POST') {
          state.installCalls++;
          assert.equal(parse(state.content).agent_servers['codex-acp'].type, 'registry');
          if (state.installGate) await state.installGate;
          if (state.installError) { await route.fulfill({ status: 400, json: { message: state.installError } }); return; }
          state.installed = true;
        }
        data = state.installStatus;
      } else if (path === '/api/acp/connection-status') data = state.connectionStatus;
      else if (/^\/api\/acp\/sessions\/[^/]+\/connection$/.test(path)) data = { id: 'connection', agent_id: 'test', cwd: '/repo', auth_methods: state.authRequired || state.advertiseAuth ? [{ id: 'login', name: '登录 Agent', type: 'agent' }] : [], capabilities: state.capabilities || { loadSession: true }, prompt_capabilities: state.imageSupport === false ? {} : { image: {} } };
      else if (path === '/api/acp/connections') {
        state.connections++;
        if (state.connectionGate) await state.connectionGate;
        if (state.connectionError) { await route.fulfill({ status: 400, json: { message: state.connectionError } }); return; }
        data = { id: 'connection', agent_id: 'test', cwd: '/repo', auth_methods: state.authRequired || state.advertiseAuth ? [{ id: 'login', name: '登录 Agent', type: 'agent' }] : [], capabilities: state.capabilities || { loadSession: true, sessionCapabilities: { list: {} } } };
      }
      else if (path === '/api/acp/connections/connection/authenticate') { state.authenticating = true; if (state.authGate) await state.authGate; state.authenticated = true; state.authenticating = false; for (const item of state.sessions) { item.auth_required = false; item.error = null; item.revision++; } data = {}; }
      else if (path === '/api/acp/connections/connection/requests') {
        if (request.method() === 'POST') { state.authAnswer = request.postDataJSON(); state.finishAuth?.(); data = {}; }
        else data = state.authenticating ? state.authRequests || [] : state.connectionRequests || [];
      }
      else if (path === '/api/acp/connections/connection/sessions' && request.method() === 'GET') {
        state.listCalls.push(url.searchParams.get('cursor'));
        if (state.historyError) { await route.fulfill({ status: 400, json: { message: state.historyError } }); return; }
        if (state.historyAuthRequired && !state.authenticated) { await route.fulfill({ status: 400, json: { code: 'acp_auth_required', message: 'Sign in to read history' } }); return; }
        data = state.pages?.[url.searchParams.get('cursor') || 'first'] || { sessions: state.history || [{ sessionId: 'remote-old', title: 'Earlier conversation', cwd: '/repo' }] };
      }
      else if (path === '/api/acp/connections/connection/import') {
        const selected = request.postDataJSON(); state.imports.push(selected);
        if (state.importError) { await route.fulfill({ status: 400, json: { message: state.importError } }); return; }
        data = [];
        for (const item of selected) {
          if (state.sessions.some(saved => saved.agent_id === 'test' && saved.cwd === item.cwd && saved.session_id === item.session_id)) continue;
          const imported = { ...session, ...item, thread_id: `imported-${state.sessions.length}`, agent_id: 'test', title: item.title || 'New conversation', entries: [], status: 'disconnected', needs_load: true };
          state.sessions.push(imported); data.push(imported);
        }
      }
      else if (path === '/api/acp/sessions') {
        if (request.method() === 'POST') {
          (state.createInputs ??= []).push(request.postDataJSON());
          data = state.sessions.length ? { ...session, thread_id: `local-${state.sessions.length + 1}`, title: `Conversation ${state.sessions.length + 1}` } : session;
          state.preparedStates ??= new Map();
          state.preparedStates.set(data.thread_id, { status: data.status, session_id: data.session_id });
          data.session_id = null; data.status = 'disconnected';
          state.sessions.push(data);
        } else data = state.sessions.filter(item => item.cwd === url.searchParams.get('cwd'));
      }
      else if (/^\/api\/acp\/sessions\/[^/]+$/.test(path)) {
        if (request.method() === 'DELETE') { state.deletes++; state.sessions = state.sessions.filter(item => item.thread_id !== path.split('/').at(-1)); }
        else { state.snapshots = (state.snapshots || 0) + 1; data = state.sessions.find(item => item.thread_id === path.split('/').at(-1)); }
      }
      else if (path.endsWith('/updates')) {
        (state.updateReads ??= []).push(Object.fromEntries(url.searchParams));
        data = state.updatePayload?.from_revision === Number(url.searchParams.get('revision')) ? state.updatePayload : { kind: 'unchanged' };
      }
      else if (path.endsWith('/process')) {
        (state.processReads ??= []).push(Object.fromEntries(url.searchParams));
        if (state.processGate) await state.processGate;
        if (state.processError) { await route.fulfill({ status: 400, json: { message: state.processError } }); return; }
        const details = state.processSnapshots[url.searchParams.get('turn_id')];
        data = state.processUpdatePayload?.from_revision === Number(url.searchParams.get('revision')) && url.searchParams.has('revision')
          ? state.processUpdatePayload : details.revision === Number(url.searchParams.get('revision')) && url.searchParams.has('revision')
            ? { kind: 'unchanged' } : { kind: 'snapshot', snapshot: details };
      }
      else if (path.endsWith('/logs')) { (state.logReads ??= []).push(path); data = state.logs || []; }
      else if (path.endsWith('/actions')) {
        if (!state.controls.authorized(path.split('/').at(-2), request)) {
          await route.fulfill({ status: 409, json: { code: 'acp_control_required', message: 'No session control' } }); return;
        }
        const session = state.sessions.find(item => item.thread_id === path.split('/').at(-2));
        const action = request.postDataJSON(); state.actions.push(action);
        if (action.action === 'resume' && state.preparedStates?.has(session.thread_id)) {
          state.connections++;
          if (state.connectionGate) await state.connectionGate;
          if (state.connectionError) { await route.fulfill({ status: 400, json: { message: state.connectionError } }); return; }
          if (state.authRequired && !state.authenticated) { await route.fulfill({ status: 400, json: { code: 'acp_auth_required', message: 'Authentication required' } }); return; }
          if (state.sessionError) { await route.fulfill({ status: 400, json: { code: 'acp_error', message: state.sessionError } }); return; }
          Object.assign(session, state.preparedStates.get(session.thread_id));
          state.preparedStates.delete(session.thread_id); session.revision++; state.created = true;
          await route.fulfill({ json: session }); return;
        }
        if (action.action === 'resume' && state.resumeAuthRequired && !state.authenticated) { await route.fulfill({ status: 400, json: { code: 'acp_auth_required', message: 'Sign in to restore this session' } }); return; }
        if (action.action === 'resume' && state.recoveryLifecycle) { session.recovery = { attempt_id: 'load-attempt', phase: 'loading' }; session.revision++; }
        if (action.action === 'cancel' && session.recovery?.phase === 'loading') {
          session.recovery.phase = 'cleaning'; session.revision++; state.cancelledRecovery = true;
          await route.fulfill({ json: session }); return;
        }
        if (action.action === 'resume' && state.resumeGate) await state.resumeGate;
        if (action.action === 'resume' && state.cancelledRecovery) {
          session.recovery = { attempt_id: 'load-attempt', phase: 'failed', error: 'Recovery cancelled' }; session.revision++;
          await route.fulfill({ status: 400, json: { message: 'Recovery cancelled' } }); return;
        }
        if (action.action === 'resume' && state.resumeError) { await route.fulfill({ status: 400, json: { message: state.resumeError } }); return; }
        if (action.action === 'prompt' && state.promptGate) await state.promptGate;
        if (action.action === 'prompt' && state.promptError) { await route.fulfill({ status: 400, json: { message: state.promptError } }); return; }
        session.revision++;
        if (action.action === 'prompt') {
          session.entries = [...action.content.map((content, index) => ({ id: `user-${index}`, kind: 'user', content })), { id: 'assistant', kind: 'assistant', content: { type: 'text', text: 'Hello from ACP' } }, { id: 'tool', kind: 'tool', content: { title: 'Read source', status: 'in_progress', locations: [{ path: '/repo/main.rs', line: 12 }], content: [{ type: 'diff', path: '/repo/main.rs', oldText: 'old', newText: 'new' }] } }];
          session.permissions = [{ id: 'permission', kind: 'permission', request: { toolCall: { title: 'Write source' }, options: [{ optionId: 'yes', name: 'Allow once' }] } }];
        } else if (action.action === 'answer') session.permissions = [];
        else if (action.action === 'dismiss_notice') session.notices = session.notices.filter(notice => notice.id !== action.notice_id);
        else if (action.action === 'clear_plan') session.plan = null;
        else if (action.action === 'set_config') session.config_options = session.config_options.map(option => (option.configId || option.id) === action.config_id ? { ...option, currentValue: action.value } : option);
        else if (action.action === 'set_mode') session.modes.currentModeId = action.mode_id;
        else if (action.action === 'resume') { session.recovery = null; session.status = 'idle'; if (session.needs_load) { session.needs_load = false; state.loads++; session.entries = state.loadedEntries || [{ id: 'loaded', kind: 'assistant', content: { type: 'text', text: 'Imported conversation content' } }]; } }
        data = session;
      }
      if (data?.thread_id && state.preparedStates?.has(data.thread_id)) data = { ...data, entries: [], permissions: [] };
      await route.fulfill({ json: data });
    });
    await page.goto(`${origin}/tests/acp-preview.html${settings ? '?settings' : ''}`);
    if (!settings) await page.waitForFunction(() => document.querySelector('[aria-label="刷新 ACP"]')?.disabled === false);
    return { page, state };
  }
  const interactions = state => state.actions.filter(action => action.action !== 'resume');
  const newSession = async page => { await page.getByRole('button', { name: '新建窗体', exact: true }).click(); await page.getByRole('button', { name: '新建 ACP 会话：Test Agent', exact: true }).click(); };
  const toggleThinking = async (page, checked = false) => {
    await page.getByRole('button', { name: '会话更多操作', exact: true }).click();
    const option = page.getByRole('menuitemcheckbox', { name: '显示 Thinking', exact: true });
    assert.equal(await option.getAttribute('aria-checked'), String(checked));
    await option.click();
  };
  const importHistory = async page => { await page.getByRole('button', { name: '导入 ACP 会话', exact: true }).click(); const dialog = page.getByRole('dialog', { name: '导入外部 Agent 会话' }); await dialog.getByRole('button', { name: '导入 1 个会话', exact: true }).click(); await dialog.waitFor({ state: 'detached' }); };
  await test('ACP deletion uses the shared confirmation and only deletes after explicit approval', async t => {
    const { page, state } = await fixture(t);
    const nativeDialogs = [];
    page.on('dialog', async dialog => { nativeDialogs.push(dialog.type()); await dialog.dismiss(); });
    await newSession(page);
    const remove = page.getByRole('button', { name: '删除 ACP 会话 Conversation', exact: true });
    const confirmation = page.getByRole('alertdialog', { name: '删除 ACP 会话？', exact: true });
    for (const dismiss of ['取消', 'Escape', '关闭对话框', 'backdrop']) {
      await remove.click();
      await confirmation.waitFor();
      assert.equal(state.deletes, 0, 'opening the dialog never deletes a session');
      assert.equal(await confirmation.getByRole('button', { name: '取消', exact: true }).evaluate(button => button === document.activeElement), true);
      await confirmation.getByText('删除会话“Conversation”。', { exact: true }).waitFor();
      await confirmation.getByText('支持远端删除的 Agent 也会删除远端记录。', { exact: true }).waitFor();
      if (dismiss === 'Escape') await page.keyboard.press('Escape');
      else if (dismiss === 'backdrop') await page.mouse.click(1, 1);
      else await confirmation.getByRole('button', { name: dismiss, exact: true }).click();
      await confirmation.waitFor({ state: 'detached' });
      assert.equal(state.deletes, 0, 'dismissal preserves the session');
      assert.equal(state.sessions.length, 1);
    }
    await remove.click();
    await confirmation.waitFor();
    for (const width of [1280, 390]) {
      await page.setViewportSize({ width, height: 800 });
      const bounds = await confirmation.boundingBox();
      assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= width, 'the confirmation fits desktop and phone widths');
      if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-delete-${width}.png` });
    }
    await confirmation.getByRole('button', { name: '删除', exact: true }).click();
    await confirmation.waitFor({ state: 'detached' });
    await remove.waitFor({ state: 'detached' });
    assert.equal(state.deletes, 1);
    assert.equal(state.sessions.length, 0);
    assert.equal(await page.getByRole('tab').filter({ hasText: 'Conversation' }).count(), 0, 'deleting the active session closes its tab');
    assert.deepEqual(nativeDialogs, []);
  });
  function projectedHistory(state, count = 3, running = false) {
    state.session.sync_generation = 'projection-test';
    state.session.status = running ? 'working' : 'idle';
    state.session.overview = [];
    state.processSnapshots = {};
    for (let index = 0; index < count; index++) {
      const user = { id: `question-${index}`, kind: 'user', content: { type: 'text', text: `Question ${index}` } };
      const conclusion = { id: `answer-${index}`, kind: 'assistant', content: { type: 'text', text: `Conclusion ${index}` } };
      const turn = { id: user.id, revision: 1, user: [user], conclusion: running ? [] : [conclusion], tool_count: 1, progress_count: 1,
        running, waiting: false, compacting: false, status: running ? 'in_progress' : 'completed', permission_tool_ids: [] };
      state.session.overview.push(turn);
      state.processSnapshots[turn.id] = { ...state.session, overview: undefined, entries: [user,
        { id: `thought-${index}`, kind: 'thought', content: { type: 'text', text: 'private process '.repeat(70_000) } },
        { id: `tool-${index}`, kind: 'tool', content: { title: `Command ${index}`, kind: 'execute', status: 'completed', content: [{ type: 'terminal', terminalId: `terminal-${index}` }] } }, conclusion],
      terminals: { [`terminal-${index}`]: { output: `output ${index}\r\n`.repeat(2000) } } };
    }
  }

  for (const legacyResponse of [false, true]) {
    await test(`notifications before resume completes keeps overview mode${legacyResponse ? ' after a same-revision full response' : ''}`, async t => {
      const { page, state } = await fixture(t);
      state.session.sync_generation = 'prepared';
      let ready = false;
      let release;
      const gate = new Promise(resolve => { release = resolve; });
      t.after(() => release());
      const user = { id: 'question', kind: 'user', content: { type: 'text', text: 'Question' } };
      const tool = { id: 'tool', kind: 'tool', content: { title: 'Command', kind: 'execute', status: 'completed', content: [{ type: 'terminal', terminalId: 'terminal' }] } };
      const answer = { id: 'answer', kind: 'assistant', content: { type: 'text', text: 'Resume conclusion' } };
      const full = () => ({ ...state.session, sync_generation: 'live', revision: 2, status: 'idle', entries: [user, tool, answer], terminals: { terminal: { output: 'x'.repeat(1024 * 1024) } } });
      const projected = () => ({ ...full(), entries: [], terminals: {}, overview: [{ id: 'question', revision: 2, user: [user], conclusion: [answer], tool_count: 1, progress_count: 0,
        running: false, waiting: false, compacting: false, status: 'completed', permission_tool_ids: [] }] });
      state.processSnapshots = { question: { ...full(), entries: [tool] } };
      await page.route('**/api/acp/sessions/local?*', async route => {
        await route.fulfill({ json: ready ? projected() : { ...state.session, overview: [] } });
      });
      await page.route('**/api/acp/sessions/local/actions?*', async route => {
        ready = true;
        state.preparedStates.delete('local');
        Object.assign(state.session, projected());
        await gate;
        await route.fulfill({ json: legacyResponse ? full() : projected() });
      });
      const reads = [];
      await page.route('**/api/acp/sessions/local/updates?*', async route => {
        const query = new URL(route.request().url()).searchParams;
        reads.push(Object.fromEntries(query));
        // The legacy response models an already in-flight canonical read; even
        // at the same revision, the later overview must replace its full body.
        await route.fulfill({ json: ready && query.get('generation') !== 'live'
          ? { kind: 'snapshot', snapshot: !legacyResponse && query.get('view') === 'overview' ? projected() : full() } : ready && Number(query.get('revision')) < state.session.revision ? { kind: 'snapshot', snapshot: { ...projected(), revision: state.session.revision } } : { kind: 'unchanged' } });
      });
      await newSession(page);
      await page.getByText('Resume conclusion', { exact: true }).waitFor();
      const resumed = page.waitForResponse(response => new URL(response.url()).pathname.endsWith('/actions'));
      release(); await resumed;
      const nextRead = page.waitForRequest(request => new URL(request.url()).pathname.endsWith('/updates') && new URL(request.url()).searchParams.get('generation') === 'live');
      state.session.revision++;
      await nextRead;
      await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
      await page.getByRole('button', { name: '展开工具执行：1 次调用', exact: true }).waitFor();
      assert.ok(reads.every(read => read.view === 'overview'), 'preparation never selects canonical polling');
      assert.equal(state.processReads?.length, 1, 'expanding loads scoped details after the resume race');
      await page.getByRole('button', { name: '收起处理过程', exact: true }).click();
      await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
      await page.getByRole('button', { name: '展开工具执行：1 次调用', exact: true }).waitFor();
      assert.equal(state.processReads.length, 2, 'collapse releases the previously loaded process');
    });
  }

  await test('collapsed overviews load no process and repeated collapse releases detail DOM and resources', async t => {
    const { page, state } = await fixture(t);
    projectedHistory(state);
    await newSession(page);
    await page.getByText('Conclusion 2', { exact: true }).waitFor();
    assert.equal(state.processReads?.length || 0, 0);
    assert.equal(await page.locator('.session-process-timeline, .xterm').count(), 0);
    const client = await page.context().newCDPSession(page);
    const measure = async () => {
      await client.send('HeapProfiler.collectGarbage');
      return { ...await client.send('Runtime.getHeapUsage'), ...await client.send('Memory.getDOMCounters') };
    };
    let warmed;
    for (let cycle = 0; cycle < 3; cycle++) {
      for (let index = 0; index < 3; index++) {
        const turn = page.locator('.project-aow-snapshot-turn').filter({ hasText: `Question ${index}` });
        await turn.getByRole('button', { name: '展开处理过程', exact: true }).click();
        await turn.getByRole('button', { name: '展开工具执行：1 次调用', exact: true }).click();
        await turn.getByRole('button', { name: `Command ${index}`, exact: true }).click();
        await turn.locator('.xterm').waitFor();
      }
      assert.equal(await page.locator('.xterm').count(), 3);
      await page.getByRole('button', { name: '收起处理过程', exact: true }).first().click();
      await page.getByRole('button', { name: '收起处理过程', exact: true }).first().click();
      await page.getByRole('button', { name: '收起处理过程', exact: true }).first().click();
      assert.equal(await page.locator('.session-process-timeline, .xterm, .monaco-editor').count(), 0);
      const measured = await measure();
      if (!warmed) warmed = measured;
      else {
        assert.ok(measured.usedSize < warmed.usedSize + 8 * 1024 * 1024, 'collapsed process bodies must not accumulate');
        assert.ok(measured.nodes < warmed.nodes + 200, 'terminal DOM must be released');
        assert.ok(measured.jsEventListeners < warmed.jsEventListeners + 30, 'detail listeners must be released');
      }
      t.diagnostic(JSON.stringify({ cycle, heap: measured.usedSize, nodes: measured.nodes, listeners: measured.jsEventListeners }));
    }
    assert.equal(state.processReads.length, 9);
    assert.ok(state.processReads.every(read => read.revision === undefined), 'reopening fetches a fresh detail after release');
  });

  await test('overview patches retain unchanged prompts and reject missing initial turn content', async () => {
    const { applySessionUpdates } = await server.ssrLoadModule('/src/features/acp/session_updates.ts');
    const state = { session: { thread_id: 'local', entries: [], revision: 1 } };
    projectedHistory(state, 2, true);
    const before = state.session;
    const turn = { ...before.overview[1], revision: 2, progress_count: 2 };
    delete turn.user;
    const update = { kind: 'overview', from_revision: 1, snapshot: { ...before, overview: undefined, revision: 2 }, turns: [turn] };
    const next = applySessionUpdates(before, update);
    assert.equal(next.overview[0], before.overview[0]);
    assert.equal(next.overview[1].user, before.overview[1].user);
    assert.equal(next.overview[1].progress_count, 2);
    assert.equal(applySessionUpdates(before, { ...update, turns: [{ ...turn, id: 'unknown' }] }), undefined);
    assert.equal(before.overview[1].progress_count, 1, 'failed patches are atomic');
  });

  await test('projected leading history retains conclusions, copying and navigation without a user prompt', async t => {
    const { page, state } = await fixture(t);
    projectedHistory(state, 1);
    state.session.overview[0].id = 'leading';
    state.session.overview[0].user = [];
    state.session.overview[0].tool_count = 0;
    state.session.overview[0].progress_count = 0;
    await newSession(page);
    await page.getByText('Conclusion 0', { exact: true }).waitFor();
    assert.equal(await page.locator('.project-aow-snapshot-turn').count(), 0);
    await page.getByRole('button', { name: '复制回复', exact: true }).waitFor();
    await page.getByRole('button', { name: '跳到用户消息', exact: true }).click();
    await page.getByRole('button', { name: '回到顶部', exact: true }).click();
    assert.equal(state.processReads?.length || 0, 0);
  });

  await test('collapsing an active process stops detail reads and reopening gets current output', async t => {
    const { page, state } = await fixture(t);
    projectedHistory(state, 1, true);
    await newSession(page);
    await page.getByRole('button', { name: 'Command 0', exact: true }).waitFor();
    await page.getByRole('button', { name: '收起处理过程', exact: true }).click();
    const reads = state.processReads.length;
    const from = state.session.revision;
    state.session.revision++;
    state.session.overview[0].revision = state.session.revision;
    state.processSnapshots['question-0'].revision = state.session.revision;
    state.processSnapshots['question-0'].entries.at(-1).content.text = 'Latest progress after collapse';
    state.updatePayload = { kind: 'overview', from_revision: from, snapshot: { ...state.session, overview: undefined }, turns: state.session.overview };
    await page.waitForFunction(() => document.querySelector('[aria-label="展开处理过程"]'));
    await new Promise(resolve => setTimeout(resolve, 1600));
    assert.equal(state.processReads.length, reads, 'folded active process must not fetch its body');
    assert.equal(await page.locator('.session-process-timeline').count(), 0);
    await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
    await page.getByText('Latest progress after collapse', { exact: true }).waitFor();
    assert.equal(state.processReads.length, reads + 1);
  });

  await test('collapsed process cancels pending loads and failed details can retry', async t => {
    const { page, state } = await fixture(t);
    projectedHistory(state, 1);
    await newSession(page);
    state.processGate = new Promise(resolve => { state.finishProcess = resolve; });
    await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
    await page.getByText('正在加载处理过程…', { exact: true }).waitFor();
    const failed = page.waitForEvent('requestfailed', request => request.url().includes('/process?'));
    await page.getByRole('button', { name: '收起处理过程', exact: true }).click();
    await failed;
    state.finishProcess(); state.processGate = undefined;
    assert.equal(await page.locator('.session-process-timeline').count(), 0);
    state.processError = 'Detail temporarily unavailable';
    await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: state.processError }).waitFor();
    state.processError = '';
    await page.getByRole('button', { name: '重试加载处理过程', exact: true }).click();
    await page.getByRole('button', { name: '展开工具执行：1 次调用', exact: true }).waitFor();
  });

  await test('ACP deltas preserve history references and reject stale bases without partial mutation', async () => {
    const { applySessionUpdates } = await server.ssrLoadModule('/src/features/acp/session_updates.ts');
    const previous = { thread_id: 't', sync_generation: 'g', revision: 7, entries: [
      { id: 'old', content: { text: 'history' } }, { id: 'live', content: { text: '你好' } },
    ], terminals: { terminal: { output: 'first' }, old: { output: 'history' } } };
    const delta = { kind: 'delta', thread_id: 't', sync_generation: 'g', from_revision: 7, revision: 8, updated_at: '', fields: {},
      entries: [{ kind: 'append_text', index: 1, id: 'live', text: '🌍' }], terminals: [{ kind: 'append_output', id: 'terminal', text: 'second' }] };
    const next = applySessionUpdates(previous, delta);
    assert.equal(next.entries[0], previous.entries[0]);
    assert.equal(next.terminals.old, previous.terminals.old);
    assert.equal(next.entries[1].content.text, '你好🌍');
    assert.equal(next.terminals.terminal.output, 'firstsecond');
    assert.equal(previous.entries[1].content.text, '你好');
    assert.equal(applySessionUpdates(next, delta), next, 'duplicate responses do not replay appends');
    assert.equal(applySessionUpdates(previous, { ...delta, from_revision: 6 }), undefined);
    assert.equal(applySessionUpdates(previous, { ...delta, sync_generation: 'other' }), undefined);
    assert.equal(applySessionUpdates(previous, { ...delta, terminals: [{ kind: 'append_output', id: 'missing', text: 'bad' }] }), undefined);
    assert.equal(previous.entries[1].content.text, '你好', 'invalid later patch cannot mutate earlier entries');
  });
  await test('ACP overview and process notifications use cursors without downloading history again', async t => {
    const { page, state } = await fixture(t);
    const user = { id: 'user', kind: 'user', content: { type: 'text', text: 'Question' } };
    const turn = { id: 'user', revision: 1, user: [user], conclusion: [], tool_count: 0, progress_count: 1,
      running: true, waiting: false, compacting: false, status: 'in_progress', permission_tool_ids: [] };
    Object.assign(state.session, { sync_generation: 'generation', status: 'working', entries: [], terminals: {}, overview: [turn] });
    state.processSnapshots = { user: { ...state.session, overview: undefined, entries: [
      { id: 'answer', kind: 'assistant', content: { type: 'text', text: 'Streaming answer' } },
    ] } };
    const resumed = page.waitForResponse(response => new URL(response.url()).pathname.endsWith('/actions'));
    await newSession(page);
    await resumed;
    await page.getByText('Streaming answer', { exact: true }).waitFor();
    const snapshots = state.snapshots;
    const revision = state.session.revision;
    state.updatePayload = { kind: 'overview', from_revision: revision, snapshot: { ...state.session, overview: undefined, revision: revision + 1 }, turns: [{ ...turn, revision: revision + 1 }] };
    state.processUpdatePayload = { kind: 'delta', thread_id: 'local', sync_generation: 'generation', from_revision: state.processSnapshots.user.revision, revision: revision + 1, updated_at: '', fields: {},
      entries: [{ kind: 'append_text', index: 0, id: 'answer', text: ' completed' }], terminals: [] };
    await page.getByText('Streaming answer completed', { exact: true }).waitFor();
    assert.equal(state.snapshots, snapshots);
    assert.ok(state.updateReads.some(read => read.view === 'overview' && read.generation === 'generation' && Number(read.revision) === revision));
    assert.equal(state.processReads.length, 2);
    assert.equal(state.processReads[1].generation, 'generation');
    assert.equal(Number(state.processReads[1].revision), state.processSnapshots.user.revision);
  });
  await test('ACP final candidates respect message identities, multipart replies and tool-only endings', async () => {
    const { finalMessageEntries, turnPresentation } = await server.ssrLoadModule('/src/features/acp/agent_ui/conversation_view/conversation_turns.ts');
    const { messageGroups } = await server.ssrLoadModule('/src/features/acp/agent_ui/conversation_view/entry_view_state.ts');
    const entry = (id, kind, message_id, content) => ({ id, kind, message_id, content });
    const progress = entry('p', 'assistant', 'progress', { type: 'text', text: 'Checking files' });
    const reply = entry('a', 'assistant', 'answer', { type: 'text', text: '**All done**' });
    const attachment = entry('image', 'assistant', 'answer', { type: 'image', data: 'image' });
    assert.deepEqual(finalMessageEntries([progress, reply, attachment]).map(item => item.id), ['a', 'image']);
    const legacy = [progress, reply].map(item => ({ ...item, message_id: undefined }));
    assert.deepEqual(finalMessageEntries(legacy).map(item => item.id), ['p', 'a'], 'missing boundaries preserve the whole final text block');
    for (const kind of ['tool', 'thought', 'summary', 'compaction_update']) {
      assert.deepEqual(finalMessageEntries([reply, entry('later', kind, undefined, { type: 'text', text: 'Later activity' })]), []);
    }
    const session = { status: 'idle', permissions: [], stop_reason: 'end_turn' };
    for (const status of ['failed', 'interrupted', 'in_progress']) {
      const result = turnPresentation(messageGroups([reply], false, false), [reply], session, false, { status });
      assert.equal(result.conclusion.length, 0, `${status} history never becomes a final answer`);
    }
    const users = [entry('u1', 'user', undefined, { type: 'text', text: 'First' }), entry('u2', 'user', undefined, { type: 'text', text: 'Second' })];
    assert.equal(messageGroups(users, false, false, new Set(), new Set(['u1', 'u2'])).length, 2, 'recorded turns separate consecutive prompts without replies');
  });
  for (const overview of [false, true]) {
    await test(`ACP hides thinking by default and merges tools across it in ${overview ? 'loaded history' : 'live sessions'}`, async t => {
      const { page, state } = await fixture(t);
      const entry = (id, kind, text) => ({ id, kind, content: { type: 'text', text } });
      const tool = id => ({ id, kind: 'tool', content: { title: id, kind: 'execute', status: 'completed', rawOutput: `${id} output` } });
      const user = entry('u', 'user', 'Inspect the project');
      const conclusion = entry('final', 'assistant', 'Inspection finished');
      const entries = [user, tool('tool-1'), entry('thought-1', 'thought', 'First thought'), tool('tool-2'), tool('tool-3'),
        entry('thought-2', 'thought', 'Second thought'), tool('tool-4'), entry('thought-3', 'thought', 'Third thought'),
        entry('thought-4', 'thought', 'Fourth thought'), tool('tool-5'), entry('mixed-before', 'thought', 'Before progress'),
        entry('progress', 'assistant', 'Ordinary progress stays visible'), entry('mixed-after', 'thought', 'After progress'), tool('tool-6'), conclusion];
      Object.assign(state.session, { stop_reason: 'end_turn', entries });
      if (overview) {
        state.session.sync_generation = 'thinking-test';
        state.session.overview = [{ id: user.id, revision: 1, user: [user], conclusion: [conclusion], tool_count: 6, progress_count: 4,
          running: false, waiting: false, compacting: false, status: 'completed', permission_tool_ids: [] }];
        state.processSnapshots = { u: { ...state.session, overview: undefined } };
        state.session.entries = [];
      }
      await newSession(page);
      await page.getByText('Inspection finished', { exact: true }).waitFor();
      await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
      const merged = page.getByRole('button', { name: '展开工具执行：5 次调用', exact: true });
      await merged.waitFor();
      assert.equal(await page.locator('.acp-thinking, .session-process-marker .lucide-brain').count(), 0);
      assert.deepEqual(await page.locator('.session-tool-group-count').allTextContents(), ['5 次调用', '1 次调用']);
      assert.equal(await page.getByText('Ordinary progress stays visible', { exact: true }).isVisible(), true);
      await merged.click();
      assert.deepEqual(await page.locator('.acp-process-tool-group').first().getByRole('region').evaluateAll(nodes => nodes.map(node => node.getAttribute('aria-label'))), ['tool-1', 'tool-2', 'tool-3', 'tool-4', 'tool-5']);
      const reads = state.processReads?.length ?? 0;
      const actions = structuredClone(interactions(state));
      await toggleThinking(page);
      await page.getByRole('button', { name: '思考过程', exact: true }).first().waitFor();
      assert.equal(await page.locator('.acp-thinking').count(), 6);
      assert.deepEqual(await page.locator('.session-tool-group-count').allTextContents(), ['1 次调用', '2 次调用', '1 次调用', '1 次调用', '1 次调用']);
      assert.equal(await page.getByText('Ordinary progress stays visible', { exact: true }).isVisible(), true);
      await toggleThinking(page, true);
      await page.getByRole('button', { name: /工具执行：5 次调用$/ }).waitFor();
      assert.equal(await page.locator('.acp-thinking').count(), 0);
      assert.equal(await page.locator('.project-aow-snapshot-conclusion').innerText(), 'Inspection finished');
      assert.equal(state.processReads?.length ?? 0, reads, 'visibility changes reuse the loaded process');
      assert.deepEqual(interactions(state), actions, 'visibility does not send agent actions');
      assert.equal((overview ? state.processSnapshots.u.entries : state.session.entries).filter(entry => entry.kind === 'thought').length, 6);
      if (!overview) {
        state.session.entries.push(entry('late-thought', 'thought', 'Later activity'));
        state.session.revision++;
        await page.locator('.project-aow-snapshot-conclusion').waitFor({ state: 'detached' });
        assert.equal(await page.locator('.acp-thinking').count(), 0, 'streamed thoughts remain hidden');
        assert.equal(await page.getByText('Inspection finished', { exact: true }).isVisible(), true, 'hiding thoughts must not promote earlier progress to a final reply');
      }
    });
  }
  await test('ACP process and consecutive tool groups expand live and collapse on completion', async t => {
    const { page, state } = await fixture(t);
    const message = (id, text) => ({ id, message_id: id, kind: 'assistant', content: { type: 'text', text } });
    const tool = (id, title) => ({ id, kind: 'tool', content: { title, kind: 'execute', status: 'completed', rawOutput: `${title} output` } });
    Object.assign(state.session, { status: 'working', turns: [{ user_entry_id: 'u', status: 'in_progress' }], entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Check the project' } },
      message('progress', 'Checking source files'), tool('t1', 'pwd'), tool('t2', 'ls'),
      message('verification', 'Running verification'), tool('t3', 'npm test'),
    ] });
    await newSession(page);
    const response = page.locator('.acp-turn-response');
    const processToggle = response.locator('.session-process-toggle');
    const groups = response.locator('.session-tool-group-toggle');
    await groups.first().waitFor();
    assert.equal(await processToggle.getAttribute('aria-expanded'), 'true');
    assert.deepEqual(await groups.evaluateAll(buttons => buttons.map(button => button.getAttribute('aria-expanded'))), ['true', 'true']);
    assert.deepEqual(await response.locator('.session-tool-group-count').allTextContents(), ['2 次调用', '1 次调用']);
    await groups.first().click();
    state.session.entries.splice(3, 0, tool('t4', 'git status')); state.session.revision++;
    await page.waitForFunction(() => document.querySelector('.session-tool-group-count')?.textContent === '3 次调用');
    assert.equal(await groups.first().getAttribute('aria-expanded'), 'false', 'streaming preserves a manual collapse');
    state.session.entries.push(message('final', 'All checks passed')); state.session.revision++;
    await response.getByText('All checks passed', { exact: true }).waitFor();
    assert.equal(await response.locator('.project-aow-snapshot-conclusion').count(), 0, 'streaming text is provisional until the turn finishes');
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-process-running.png` });
    state.session.status = 'idle'; state.session.stop_reason = 'end_turn';
    state.session.turns[0] = { user_entry_id: 'u', status: 'completed', stop_reason: 'end_turn' }; state.session.revision++;
    await response.locator('.project-aow-snapshot-conclusion').getByText('All checks passed', { exact: true }).waitFor();
    await page.waitForFunction(() => document.querySelector('.session-process-toggle')?.getAttribute('aria-expanded') === 'false');
    assert.equal(await response.getByText('Checking source files', { exact: true }).isVisible(), false);
    assert.equal(await groups.count(), 0, 'completion unmounts process children');
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-process-completed.png` });
    await processToggle.click();
    assert.deepEqual(await groups.evaluateAll(buttons => buttons.map(button => button.getAttribute('aria-expanded'))), ['false', 'false']);
    assert.equal(await response.getByText('Checking source files', { exact: true }).isVisible(), true);
    assert.equal(await response.getByRole('region', { name: 'pwd', exact: true }).isVisible(), false);
    await groups.first().click();
    const command = response.getByRole('region', { name: 'pwd', exact: true });
    await command.getByRole('button', { name: 'pwd', exact: true }).click();
    await command.getByText('pwd output', { exact: true }).waitFor();
    state.session.revision++;
    await page.waitForTimeout(900);
    assert.equal(await processToggle.getAttribute('aria-expanded'), 'true', 'finished polling preserves a manual expansion');
    await page.locator('aside').evaluate(element => { element.style.display = 'none'; });
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await page.locator('.acp-conversation').evaluate(element => element.scrollWidth <= element.clientWidth + 1), true);
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-process-mobile.png` });
    await page.reload();
    await page.getByRole('tab').first().click();
    await response.locator('.project-aow-snapshot-conclusion').getByText('All checks passed', { exact: true }).waitFor();
    assert.equal(await processToggle.getAttribute('aria-expanded'), 'false');
    assert.equal(await groups.count(), 0, 'reloaded collapsed history mounts no process children');
  });
  await test('ACP pending permissions reveal collapsed process and tool groups without losing input', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Edit the file' } },
      { id: 'edit', kind: 'tool', content: { title: 'Apply changes', kind: 'edit', status: 'pending', content: [{ type: 'diff', path: '/repo/main.rs', oldText: 'before', newText: 'after' }] } },
    ] });
    await newSession(page);
    const processToggle = page.locator('.session-process-toggle');
    const group = page.locator('.session-tool-group-toggle');
    await group.click(); await processToggle.click();
    state.session.permissions = [{ id: 'p', kind: 'permission', request: { toolCall: { toolCallId: 'edit', title: 'Apply changes' }, options: [{ optionId: 'yes', name: 'Allow once' }] } }]; state.session.revision++;
    await page.getByRole('button', { name: 'Allow once', exact: true }).waitFor();
    assert.equal(await processToggle.getAttribute('aria-expanded'), 'true');
    assert.equal(await group.getAttribute('aria-expanded'), 'true');
    assert.equal(await processToggle.isDisabled(), true);
    assert.equal(await group.isDisabled(), true);
    await page.getByLabel('ACP 消息', { exact: true }).fill('Keep my next prompt');
    await page.getByRole('button', { name: 'Allow once', exact: true }).click();
    assert.equal(state.actions.at(-1).request_id, 'p');
    assert.equal(await page.getByLabel('ACP 消息', { exact: true }).innerText(), 'Keep my next prompt');
  });
  await test('ACP historical failures retain outcomes and never promote progress to a conclusion', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { turns: [
      { user_entry_id: 'u1', status: 'interrupted', stop_reason: 'cancelled' },
      { user_entry_id: 'u2', status: 'failed', error: 'Agent crashed' },
      { user_entry_id: 'u3', status: 'completed', stop_reason: 'end_turn' },
    ], entries: [
      { id: 'u1', kind: 'user', content: { type: 'text', text: 'Cancelled prompt' } },
      { id: 'a1', kind: 'assistant', content: { type: 'text', text: 'Still checking' } },
      { id: 'u2', kind: 'user', content: { type: 'text', text: 'Failed prompt' } },
      { id: 'u3', kind: 'user', content: { type: 'text', text: 'Latest prompt' } },
      { id: 'tool', kind: 'tool', content: { title: 'Last tool', status: 'completed' } },
    ] });
    await newSession(page);
    await page.getByText('本轮已中断，未生成最终结论。', { exact: true }).waitFor();
    await page.getByText('Agent crashed', { exact: true }).waitFor();
    assert.equal(await page.locator('.project-aow-snapshot-turn').count(), 3);
    assert.equal(await page.locator('.project-aow-snapshot-conclusion').count(), 0);
    assert.equal(await page.getByText('Still checking', { exact: true }).isVisible(), false);
    await page.getByText('本轮未记录可识别的最终结论。', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '复制回复', exact: true }).count(), 0);
    await page.reload();
    await page.getByRole('tab').first().click();
    await page.getByText('本轮已中断，未生成最终结论。', { exact: true }).waitFor();
    assert.equal(await page.locator('.project-aow-snapshot-conclusion').count(), 0);
  });
  await test('workspace ACP menu filters unavailable agents and refreshes after settings change', async t => {
    const agents = [
      { id: 'test', name: 'Test Agent', supported: true, installed: true },
      { id: 'missing', name: 'Missing Agent', supported: true, installed: false },
      { id: 'unsupported', name: 'Unsupported Agent', supported: false, installed: true },
    ];
    const { page, state } = await fixture(t, false, { agents });
    assert.equal(await page.getByRole('button', { name: '新建 ACP 会话', exact: true }).count(), 0);
    await page.getByText('暂无会话，请在工作区标题栏的“新建窗体”菜单中选择 ACP Agent。', { exact: true }).waitFor();
    await page.getByRole('button', { name: '新建窗体', exact: true }).click();
    const menu = page.locator('.project-aow-new-menu');
    await menu.getByRole('button', { name: '新建 ACP 会话：Test Agent', exact: true }).waitFor();
    assert.deepEqual(await menu.locator('.project-aow-menu-label').allTextContents(), ['Workspace', 'Agents', 'ACP', 'Notes', 'Files']);
    assert.equal(await menu.getByRole('button', { name: /^新建 ACP 会话：/ }).count(), 1);
    state.agents = [];
    await page.evaluate(() => window.dispatchEvent(new Event('aow:acp-settings-changed')));
    await menu.getByText('暂无可用 ACP Agent', { exact: false }).waitFor();
    assert.equal(await menu.getByRole('button', { name: /^新建 ACP 会话：/ }).count(), 0);
    state.agents = agents;
    await page.evaluate(() => window.dispatchEvent(new Event('aow:acp-settings-changed')));
    await menu.getByRole('button', { name: '新建 ACP 会话：Test Agent', exact: true }).click();
    await menu.waitFor({ state: 'detached' });
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.deepEqual(state.createInputs, [{ agent_id: 'test', cwd: '/repo' }]);
  });
  await test('workspace ACP menu reports loading and allows retry after an agent request fails', async t => {
    const { page, state } = await fixture(t);
    let release;
    state.agentsGate = new Promise(resolve => { release = resolve; });
    t.after(() => release());
    state.agentsError = 'Agent service unavailable';
    await page.getByRole('button', { name: '新建窗体', exact: true }).click();
    const menu = page.locator('.project-aow-new-menu');
    await menu.getByRole('status').filter({ hasText: '正在加载 ACP Agent…' }).waitFor();
    assert.equal(await menu.getByRole('button', { name: /^新建 ACP 会话：/ }).count(), 0);
    release();
    await menu.getByRole('alert').filter({ hasText: 'ACP Agent 加载失败' }).waitFor();
    state.agentsError = '';
    await menu.getByRole('button', { name: '重新加载 ACP Agent', exact: true }).click();
    await menu.getByRole('button', { name: '新建 ACP 会话：Test Agent', exact: true }).waitFor();
    assert.equal(state.sessions.length, 0);
  });
  await test('only the controlling page restores and writes; takeover preserves live work and drafts', async t => {
    const { page: first, state } = await fixture(t);
    state.session.status = 'disconnected'; state.sessions.push(state.session);
    await first.getByRole('button', { name: '刷新 ACP', exact: true }).click();
    await first.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    const input = first.getByLabel('ACP 消息', { exact: true });
    await input.fill('Owner draft');
    assert.deepEqual(state.actions.map(action => action.action), ['resume']);
    const claims = state.controls.claims.length;
    await first.getByRole('button', { name: '移动会话视图', exact: true }).click();
    assert.equal(await input.innerText(), 'Owner draft');
    assert.equal(state.controls.claims.length, claims, 'portal remount retains the control attachment');
    const { page: second } = await fixture(t, false, { sharedState: state });
    await second.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(await second.getByLabel('ACP 消息', { exact: true }).count(), 0);
    assert.equal(state.actions.length, 1, 'observers never restore an already-open session');
    const stale = await first.evaluate(async () => (await import('/src/features/acp-control/sessionControl.ts')).acpControlHeaders('local'));
    Object.assign(state.session, { status: 'working', revision: state.session.revision + 1,
      entries: [{ id: 'live', kind: 'assistant', content: { type: 'text', text: 'Still working' } }],
      permissions: [{ id: 'approval', kind: 'permission', request: { options: [{ optionId: 'allow', name: 'Allow once' }] } }] });
    await second.getByText('Still working', { exact: true }).waitFor();
    assert.equal(await second.getByRole('button', { name: 'Allow once', exact: true }).isDisabled(), true);
    await second.getByRole('button', { name: '接管', exact: true }).click();
    await second.getByLabel('ACP 消息', { exact: true }).fill('New owner draft');
    await first.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(await input.count(), 0);
    assert.equal(state.session.status, 'working');
    assert.equal(state.actions.length, 1, 'takeover neither resumes nor cancels live work');
    assert.equal(await second.getByRole('button', { name: 'Allow once', exact: true }).isEnabled(), true);
    assert.equal(await first.evaluate(async headers => (await fetch('/api/acp/sessions/local/actions', { method: 'POST', headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify({ action: 'cancel' }) })).status, stale), 409);
    await first.getByRole('button', { name: '接管', exact: true }).click();
    await input.waitFor();
    assert.equal(await input.innerText(), 'Owner draft');
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    await first.close();
    assert.equal(await second.getByLabel('ACP 消息', { exact: true }).count(), 0, 'vacancy does not make an observer seize control');
    await second.getByRole('button', { name: '接管', exact: true }).click();
    await second.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await second.getByLabel('ACP 消息', { exact: true }).innerText(), 'New owner draft');
  });
  await test('data reconnect refreshes a disconnected session without restoring tasks or changing control', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).fill('Preserve this draft');
    const actions = state.actions.length;
    const claims = state.controls.claims.length;
    disconnectAcpEvents(page);
    state.session.status = 'disconnected'; state.session.revision++;
    await page.getByRole('button', { name: '重试连接', exact: true }).waitFor();
    assert.equal(state.actions.length, actions, 'data resynchronization never resumes a task');
    assert.equal(state.controls.claims.length, claims, 'data resynchronization never claims control');
    assert.equal(await page.getByLabel('ACP 消息', { exact: true }).innerText(), 'Preserve this draft');
  });
  await test('control reconnects without making an observer reclaim a vacant session', async t => {
    const { page: first, state } = await fixture(t);
    await newSession(first);
    await first.getByLabel('ACP 消息', { exact: true }).fill('Reconnect draft');
    state.controls.disconnect('local');
    await first.getByText('控制连接已断开，正在重连…', { exact: true }).waitFor();
    await first.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await first.getByLabel('ACP 消息', { exact: true }).innerText(), 'Reconnect draft');
    assert.equal(interactions(state).length, 0, 'reconnecting control does not reload a live adapter');
    const { page: second } = await fixture(t, false, { sharedState: state });
    await second.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    await first.close();
    const claims = state.controls.claims.length;
    state.controls.disconnect('local');
    await second.getByText('控制连接已断开，正在重连…', { exact: true }).waitFor();
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(state.controls.claims.length, claims, 'an observer reconnects with observe, never claim');
    assert.equal(await second.getByLabel('ACP 消息', { exact: true }).count(), 0);
    await second.getByRole('button', { name: '接管', exact: true }).click();
    await second.getByLabel('ACP 消息', { exact: true }).waitFor();
  });
  await test('authentication belongs to the current owner and an old login prompt does not survive takeover', async t => {
    const { page: first, state } = await fixture(t, false, { advertiseAuth: true, resumeAuthRequired: true });
    await newSession(first);
    await first.getByLabel('ACP 消息', { exact: true }).fill('Draft before login');
    Object.assign(state.session, { status: 'disconnected', auth_required: false, revision: state.session.revision + 1 });
    await first.getByRole('button', { name: '重试连接', exact: true }).click();
    await first.getByRole('region', { name: 'Agent 登录', exact: true }).waitFor();
    const { page: second } = await fixture(t, false, { sharedState: state });
    await second.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(await second.getByRole('region', { name: 'Agent 登录', exact: true }).count(), 0);
    await second.getByRole('button', { name: '接管', exact: true }).click();
    await first.getByRole('button', { name: '接管', exact: true }).waitFor();
    await second.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await second.getByLabel('ACP 消息', { exact: true }).fill('Authenticated owner');
    await first.getByRole('button', { name: '接管', exact: true }).click();
    await first.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await first.getByLabel('ACP 消息', { exact: true }).innerText(), 'Draft before login');
    assert.equal(await first.getByRole('region', { name: 'Agent 登录', exact: true }).count(), 0);
    assert.equal(await first.getByRole('region', { name: 'ACP 会话内容', exact: true }).getByRole('alert').count(), 0);
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'resume', 'resume', 'resume']);
  });
  await test('only the owner loads imported history while observers watch the result', async t => {
    const { page: first, state } = await fixture(t);
    await importHistory(first);
    state.resumeGate = new Promise(resolve => { state.finishResume = resolve; });
    t.after(() => state.finishResume());
    await first.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).click();
    await first.getByText(/正在恢复会话…/).waitFor();
    const { page: second } = await fixture(t, false, { sharedState: state });
    await second.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).click();
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(state.actions.filter(action => action.action === 'resume').length, 1);
    state.finishResume();
    await second.getByText('Imported conversation content', { exact: true }).waitFor();
    assert.equal(await second.getByLabel('ACP 消息', { exact: true }).count(), 0);
  });
  await test('composer follows transcript edges with reserved and overlay scrollbars', async t => {
    const { page, state } = await fixture(t);
    await page.addStyleTag({ content: '@media (max-width: 720px) { #root > div > div > aside { display: none; } }' });
    state.session.entries = [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Explain the project layout' } },
      { id: 'a', kind: 'assistant', content: { type: 'text', text: `${'A longer response to enable transcript scrolling.\n\n'.repeat(20)}Project structure:\n\n\`\`\`text\n/workspace/project/\n├── src/\n├── web/\n└── data/\n    ├── codex-home/\n    └── evidence/\n\`\`\`\n\nRun from the project directory:\n\n\`\`\`sh\ncd /workspace/project\n\`\`\`` } },
    ];
    await newSession(page);
    const scroll = page.locator('.acp-conversation');
    const input = page.getByLabel('ACP 消息', { exact: true });
    await input.fill('Keep this draft while resizing');
    const aligned = async () => {
      const chat = await page.locator('.project-aow-snapshot-chat').boundingBox();
      const composer = await page.locator('.acp-composer').boundingBox();
      assert.ok(Math.abs(chat.x - composer.x) <= 1, `left edges: chat ${chat.x}, composer ${composer.x}`);
      assert.ok(Math.abs(chat.x + chat.width - composer.x - composer.width) <= 1, `right edges: chat ${chat.x + chat.width}, composer ${composer.x + composer.width}`);
    };
    for (const gutter of ['stable', 'auto']) {
      await scroll.evaluate((element, value) => { element.style.scrollbarGutter = value; }, gutter);
      for (const width of [1680, 1000, 720, 390]) {
        await page.setViewportSize({ width, height: 900 });
        await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
        if (gutter === 'stable') assert.ok(await scroll.evaluate(element => element.offsetWidth > element.clientWidth), 'exercise a scrollbar that occupies horizontal space');
        await aligned();
        await page.getByRole('button', { name: '展开输入区', exact: true }).click();
        await aligned();
        await page.getByRole('button', { name: '收起输入区', exact: true }).click();
        assert.equal(await input.innerText(), 'Keep this draft while resizing');
        if (gutter === 'stable' && process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-composer-${width}.png` });
      }
    }
    await scroll.evaluate(element => { element.style.scrollbarGutter = 'stable'; });
    await aligned();
    state.session.entries = []; state.session.revision++;
    await scroll.waitFor({ state: 'hidden' });
    const empty = await page.locator('.acp-composer').boundingBox();
    const main = await page.locator('.conversation-main').boundingBox();
    assert.ok(Math.abs(empty.x + empty.width / 2 - main.x - main.width / 2) <= 1, 'empty sessions remain centered after the transcript is hidden');
    assert.equal(await input.innerText(), 'Keep this draft while resizing');
  });
  await test('ACP shares Conversation turns, fixed header, navigation and live composer', async t => {
    const { page, state } = await fixture(t);
    await page.setViewportSize({ width: 1440, height: 900 });
    state.session.entries = Array.from({ length: 8 }, (_, index) => [
      { id: `user-${index}`, kind: 'user', content: { type: 'text', text: `Request ${index + 1}` } },
      { id: `tool-${index}`, kind: 'tool', content: { title: `Read file ${index + 1}`, kind: 'read', status: 'completed', rawOutput: 'Preserve expanded output' } },
      { id: `answer-${index}`, kind: 'assistant', content: { type: 'text', text: `Answer ${index + 1}\n\n${'Paragraph content.\n\n'.repeat(6)}` } },
    ]).flat();
    await newSession(page);
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    const navigation = content.getByRole('navigation', { name: '用户输入导航' });
    const input = content.getByLabel('ACP 消息', { exact: true });
    await input.fill('Keep draft while navigating');
    assert.equal(await content.locator('.project-aow-snapshot-turn').count(), 8);
    assert.equal(await navigation.getByRole('button').count(), 8);
    await page.waitForFunction(() => document.querySelector('.project-aow-snapshot-navigation li:last-child button')?.getAttribute('aria-current') === 'location');
    const fixedPositions = async () => ({ header: await content.locator('.project-aow-snapshot-header').boundingBox(), composer: await content.locator('.acp-composer').boundingBox() });
    const before = await fixedPositions();
    const positionAfterClick = await navigation.getByRole('button').first().evaluate(button => {
      button.click();
      return document.querySelector('.acp-conversation').scrollTop;
    });
    assert.ok(positionAfterClick < 40, 'navigation reaches the first turn immediately');
    assert.deepEqual(await fixedPositions(), before, 'only the transcript scrolls');
    const first = content.locator('.project-aow-snapshot-turn').first();
    assert.equal(await first.locator('.user .acp-entry-user').innerText(), 'Request 1');
    assert.equal(await first.locator('.assistant .acp-entry-tool').count(), 0, 'collapsed tools are not mounted');
    assert.match(await first.locator('.assistant .acp-entry-assistant').innerText(), /Answer 1/);
    const user = await first.locator('.project-aow-snapshot-message.user').boundingBox();
    const agent = await first.locator('.project-aow-snapshot-message.assistant').boundingBox();
    assert.ok(user.x > agent.x && Math.abs(user.x + user.width - agent.x - agent.width) < 2, 'user aligns right and agent fills the conversation width');
    const tool = first.getByRole('region', { name: 'Read file 1', exact: true });
    await first.getByRole('button', { name: '展开处理过程', exact: true }).click();
    await first.getByRole('button', { name: '展开工具执行：1 次调用', exact: true }).click();
    await tool.getByRole('button', { name: 'Read file 1', exact: true }).click();
    await tool.getByText('Preserve expanded output', { exact: true }).waitFor();
    const scrolled = await content.locator('.acp-conversation').evaluate(element => element.scrollTop);
    state.session.status = 'working';
    state.session.entries.at(-1).content.text += '\n\nStreaming new content'; state.session.revision++;
    await content.getByRole('button', { name: '停止', exact: true }).waitFor();
    await page.waitForFunction(() => document.querySelector('[data-entry-id="answer-7"]')?.textContent.includes('Streaming new content'));
    assert.equal(await content.locator('.acp-conversation').evaluate(element => element.scrollTop), scrolled, 'streaming leaves historical reading in place');
    assert.equal(await tool.getByText('Preserve expanded output', { exact: true }).isVisible(), true);
    await content.getByRole('button', { name: '跳到最新消息', exact: true }).click();
    await page.waitForFunction(() => { const element = document.querySelector('.acp-conversation'); return element.scrollHeight - element.scrollTop - element.clientHeight < 2; });
    state.session.entries.at(-1).content.text += '\n\nAnother streamed paragraph'; state.session.revision++;
    await page.waitForFunction(() => { const element = document.querySelector('.acp-conversation'); return element.textContent.includes('Another streamed paragraph') && element.scrollHeight - element.scrollTop - element.clientHeight < 2; });
    await content.getByRole('button', { name: '展开输入区', exact: true }).click();
    assert.ok((await content.locator('.acp-composer').boundingBox()).height > before.composer.height * 2);
    assert.equal(await input.innerText(), 'Keep draft while navigating');
    await content.getByRole('button', { name: '收起输入区', exact: true }).click();
    state.connectionRequests = [{ id: 'connection-input', kind: 'elicitation', request: { mode: 'form', message: 'Connection input', requestedSchema: { type: 'object', properties: { name: { type: 'string', title: 'Name' } } } } }];
    const connectionForm = content.getByRole('form', { name: 'Agent 信息请求' });
    await connectionForm.getByLabel('Name', { exact: true }).fill('Retain connection input');
    const headerBeforeLogs = await content.locator('.project-aow-snapshot-header').boundingBox();
    await content.getByRole('button', { name: '会话更多操作', exact: true }).click();
    await page.getByRole('menuitem', { name: 'ACP 协议日志', exact: true }).click();
    assert.deepEqual(await content.locator('.project-aow-snapshot-header').boundingBox(), headerBeforeLogs);
    assert.equal(await connectionForm.count(), 1, 'connection requests mount only once');
    assert.equal(await connectionForm.getByLabel('Name', { exact: true }).inputValue(), 'Retain connection input');
    await content.locator('.acp-logs').getByRole('button', { name: '返回会话', exact: true }).click();
    assert.equal(await connectionForm.getByLabel('Name', { exact: true }).inputValue(), 'Retain connection input');
    state.connectionRequests = [];
    await connectionForm.waitFor({ state: 'detached' });
    assert.equal(await input.innerText(), 'Keep draft while navigating');
    assert.equal(await tool.getByText('Preserve expanded output', { exact: true }).count(), 1);
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-shared-conversation.png` });
    await page.locator('aside').evaluate(element => { element.style.display = 'none'; });
    await page.setViewportSize({ width: 390, height: 844 });
    assert.equal(await navigation.isVisible(), false);
    assert.equal(await input.isVisible(), true);
    assert.equal(await content.locator('.acp-conversation').evaluate(element => element.scrollWidth <= element.clientWidth + 1), true);
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-shared-conversation-mobile.png` });
  });
  await test('ACP layout retains unpaired history and groups multipart user content into one turn', async t => {
    const { page, state } = await fixture(t);
    state.session.entries = [
      { id: 'history', kind: 'assistant', content: { type: 'text', text: 'Imported history without its original prompt' } },
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Inspect the attachment' } },
      { id: 'attachment', kind: 'user', content: { type: 'resource_link', name: 'main.rs', uri: 'file:///repo/main.rs' } },
      { id: 'a', kind: 'assistant', content: { type: 'text', text: 'Inspection complete' } },
    ];
    await newSession(page);
    await page.getByText('Inspection complete', { exact: true }).waitFor();
    assert.equal(await page.locator('.project-aow-snapshot-turn').count(), 1);
    assert.equal(await page.locator('.project-aow-snapshot-message.user').count(), 1);
    assert.equal(await page.locator('.project-aow-snapshot-turn [data-entry-id="history"]').count(), 0);
    assert.equal(await page.locator('.project-aow-snapshot-chat > .assistant [data-entry-id="history"]').count(), 1);
    const navigation = page.getByRole('navigation', { name: '用户输入导航' });
    assert.equal(await navigation.getByRole('button').count(), 1);
    assert.match(await navigation.getByRole('button').getAttribute('title'), /Inspect the attachment\nmain.rs/);
    assert.equal(await page.locator('.project-aow-snapshot-message.user').getByRole('button', { name: '/repo/main.rs', exact: true }).count(), 1);
  });
  await test('ACP and Conversation share page typography, controls and responsive layout', async t => {
    const { page, state } = await fixture(t);
    const reply = '# Heading\n\n## Subtitle\n\n### Detail\n\nA paragraph with `inline code`.\n\n> A quotation.\n\n```sh\nprintf hello\n```\n\n| Name | Value |\n| --- | --- |\n| A | B |';
    const prompts = ['First prompt', 'Second prompt'];
    state.session.agent_id = 'codex-acp';
    state.session.entries = prompts.flatMap((prompt, index) => [
      { id: `u${index}`, kind: 'user', content: { type: 'text', text: prompt } },
      { id: `a${index}`, kind: 'assistant', content: { type: 'text', text: reply } },
    ]);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    await page.evaluate(() => {
      document.querySelector('aside').style.display = 'none';
      const host = document.querySelector('#root > div');
      // Use the same host theme; the ACP fixture otherwise supplies its own colors.
      for (const key of ['--aow-text', '--aow-line', '--aow-panel', '--aow-muted', '--aow-accent']) host.style.removeProperty(key);
    });
    const native = await browser.newPage();
    t.after(() => native.close());
    await native.goto(`${base}/tests/session-snapshot-preview.html`);
    await native.locator('.project-aow-snapshot-header').waitFor();
    await native.evaluate(({ prompts, reply }) => window.sessionPreview.update({
      snapshot: { status: 'completed', captured_at: '', turns: prompts.map((text, index) => ({
        id: `turn-${index}`, status: 'completed', user: { text, timestamp: null },
        final: { text: reply, timestamp: null }, activities: [],
      })) },
    }), { prompts, reply });
    const styles = (target, selector) => target.locator(selector).first().evaluate(element => {
      const style = getComputedStyle(element);
      return Object.fromEntries(['display', 'color', 'backgroundColor', 'fontSize', 'fontFamily', 'fontWeight', 'lineHeight', 'padding', 'gap', 'border', 'borderRadius', 'outline', 'overflowWrap'].map(key => [key, style[key]]));
    });
    for (const width of [1140, 800, 600, 390]) {
      await page.setViewportSize({ width, height: 900 }); await native.setViewportSize({ width, height: 900 });
      const nav = target => target.getByRole('navigation', { name: '用户输入导航' });
      if (width > 720) {
        for (const target of [page, native]) {
          await nav(target).getByRole('button').first().click();
          // Wait for React to render the active navigation state.
          await target.waitForFunction(() => document.querySelector('.project-aow-snapshot-conversation').scrollTop < 40
            && document.querySelector('.project-aow-snapshot-navigation button')?.getAttribute('aria-current') === 'location');
        }
      }
      for (const selector of ['.aow-conversation-layout', '.project-aow-snapshot-header', '.project-aow-snapshot-heading h2', '.project-aow-snapshot-workspace', '.project-aow-snapshot-latest', '.project-aow-snapshot-header > button:last-child', '.project-aow-snapshot-conversation', '.project-aow-snapshot-turn', '.project-aow-snapshot-message.user', '.project-aow-snapshot-message-header', '.project-aow-snapshot-message-header > strong', '.project-aow-snapshot-bubble', '.project-aow-snapshot-navigation', '.project-aow-snapshot-navigation > header', '.project-aow-snapshot-navigation button', '.conversation-prose', '.conversation-prose h1', '.conversation-prose h2', '.conversation-prose h3', '.conversation-prose blockquote', '.conversation-prose pre', '.conversation-prose table']) {
        assert.deepEqual(await styles(page, selector), await styles(native, selector), `${width}px: ${selector}`);
      }
      for (const target of [page, native]) {
        assert.equal(await target.locator('.project-aow-snapshot-turn-label').count(), 0);
        assert.equal((await target.locator('.project-aow-snapshot-header').boundingBox()).height, 48);
        assert.equal(await nav(target).isVisible(), width > 720);
      }
      if (width > 720) {
        assert.equal((await nav(page).boundingBox()).width, (await nav(native).boundingBox()).width);
        assert.equal((await nav(page).getByRole('button').first().boundingBox()).height, (await nav(native).getByRole('button').first().boundingBox()).height);
      }
      assert.equal(await page.getByLabel('ACP 消息', { exact: true }).isVisible(), true);
    }
  });
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
    const login = content.getByRole('region', { name: 'Agent 登录', exact: true });
    await content.getByRole('button', { name: '重试连接', exact: true }).click();
    await login.waitFor();
    const pollAgain = async () => {
      for (let index = 0; index < 3; index++) {
        const updated = page.waitForResponse(response => new URL(response.url()).pathname.endsWith('/api/acp/sessions/local') && response.request().method() === 'GET');
        state.session.revision++;
        await updated;
      }
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
    assert.equal(await input.innerText(), 'Keep my recovery draft');
    await page.waitForFunction(() => document.querySelector('[aria-label="ACP 消息"]')?.getAttribute('aria-disabled') === 'false');
    assert.equal(state.sessions.length, 1);
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'resume', 'resume']);
    // A successful login in the import dialog must also release this request-only auth state.
    state.authenticated = false; state.historyAuthRequired = true;
    state.session.status = 'disconnected'; state.session.revision++;
    await content.getByRole('button', { name: '重试连接', exact: true }).click();
    await login.waitFor();
    const sidebar = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    await sidebar.getByRole('button', { name: '导入 ACP 会话', exact: true }).click();
    await page.getByRole('dialog').getByRole('button', { name: '登录 Agent', exact: true }).click();
    await login.waitFor({ state: 'detached' });
    await page.getByRole('dialog').getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(await input.innerText(), 'Keep my recovery draft');
    await page.waitForFunction(() => document.querySelector('[aria-label="ACP 消息"]')?.getAttribute('aria-disabled') === 'false');
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'resume', 'resume', 'resume', 'resume']);
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
    assert.equal(await input.innerText(), 'Second tab draft');
    await page.getByRole('tab').filter({ hasText: 'Renamed conversation' }).click();
    assert.equal(await input.innerText(), 'First tab draft');
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await input.waitFor();
    assert.equal(await input.innerText(), 'First tab draft');
    assert.equal(state.sessions.length, 2);
    assert.deepEqual(interactions(state), []);
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
      const submitted = page.waitForRequest(request => new URL(request.url()).pathname.endsWith('/actions') && request.postDataJSON().action === 'prompt');
      await page.getByRole('button', { name: '发送', exact: true }).click();
      await submitted;
      await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
      await input.waitFor();
      assert.equal(await input.innerText(), 'Send this draft');
      if (newer) await input.fill(newer);
      const accepted = page.waitForResponse(response => new URL(response.url()).pathname.endsWith('/actions'));
      finish(); await accepted;
      await page.waitForFunction(value => document.querySelector('[aria-label="ACP 消息"]')?.innerText === value, newer);
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
    assert.equal(await input.innerText(), '');
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
      const diff = document.querySelector('.acp-diff .monaco-diff-editor');
      return diff?.textContent.includes('old') && diff.textContent.includes('new');
    });
    await page.getByRole('button', { name: '/repo/main.rs:12', exact: true }).click();
    assert.equal(await page.getByLabel('Opened file').textContent(), '/repo/main.rs:12');
    await page.getByRole('button', { name: 'Allow once', exact: true }).click();
    await page.getByRole('region', { name: 'Agent 权限请求' }).waitFor({ state: 'detached' });
    assert.deepEqual(interactions(state)[0], { action: 'prompt', content: [{ type: 'text', text: 'hello' }] });
    assert.deepEqual(interactions(state)[1], { action: 'answer', request_id: 'permission', response: { outcome: { outcome: 'selected', optionId: 'yes' } } });
  });
  await test('Registry additions retain comments and unknown JSONC fields', async t => {
    const { page, state } = await fixture(t, true);
    await page.getByRole('button', { name: '浏览 ACP Registry', exact: true }).click();
    await page.getByRole('button', { name: '添加并安装', exact: true }).click();
    await page.getByText('codex-acp 已安装，可在工作区“新建窗体”菜单的 ACP 分组中新建会话。').waitFor();
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
    await page.getByText('codex-acp 已安装，可在工作区“新建窗体”菜单的 ACP 分组中新建会话。').waitFor();
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
    await page.getByRole('tab', { selected: true }).waitFor();
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByText(/正在建立 ACP 连接/).waitFor();
    assert.equal(state.created, false);
    assert.equal(state.sessions.length, 1);
    const threadId = state.sessions[0].thread_id;
    assert.equal(state.sessions[0].session_id, null);
    state.connectionError = 'Adapter connection failed';
    finish();
    await content.getByRole('alert').getByText('Adapter connection failed', { exact: true }).waitFor();
    state.connectionError = '';
    await content.getByRole('button', { name: '重试连接', exact: true }).click();
    await content.getByLabel('ACP 消息', { exact: true }).fill('Retained after retry');
    assert.equal(await page.getByRole('tab').count(), 1);
    assert.equal(state.connections, 2);
    assert.equal(state.sessions[0].thread_id, threadId);
    assert.equal(state.sessions.length, 1);
  });
  await test('authentication required for a new session remains interactive inside its tab', async t => {
    const { page, state } = await fixture(t);
    state.authRequired = true;
    await newSession(page);
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByRole('alert').getByText('Authentication required', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 1);
    assert.equal(state.sessions[0].session_id, null);
    const threadId = state.sessions[0].thread_id;
    await content.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await content.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(await content.getByRole('region', { name: 'Agent 登录', exact: true }).count(), 0);
    assert.equal(state.authenticated, true);
    assert.equal(state.sessions[0].thread_id, threadId);
    assert.equal(state.sessions.length, 1);
    assert.equal(await page.getByRole('tab').count(), 1);
  });
  await test('moving a pending ACP tab between portal hosts creates only one session', async t => {
    const { page, state } = await fixture(t);
    let finish;
    state.connectionGate = new Promise(resolve => { finish = resolve; });
    t.after(() => finish());
    await newSession(page);
    await page.waitForResponse(response => response.url().includes('/api/acp/connection-status'));
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
  await test('pending threads can be observed elsewhere and keep identity through startup and takeover', async t => {
    const { page: first, state } = await fixture(t);
    state.connectionGate = new Promise(resolve => { state.finishConnection = resolve; });
    t.after(() => state.finishConnection());
    await newSession(first);
    await first.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 1);
    const threadId = state.sessions[0].thread_id;
    assert.equal(state.sessions[0].session_id, null);
    const { page: second } = await fixture(t, false, { sharedState: state });
    await second.getByRole('button', { name: '打开 ACP 会话 Conversation', exact: true }).click();
    await second.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(await second.getByLabel('ACP 消息', { exact: true }).count(), 0);
    assert.equal(state.actions.length, 1);
    state.finishConnection();
    await first.getByLabel('ACP 消息', { exact: true }).fill('Prepared thread draft');
    await second.getByRole('button', { name: '接管', exact: true }).click();
    await second.getByLabel('ACP 消息', { exact: true }).fill('New owner');
    await first.getByRole('button', { name: '接管', exact: true }).waitFor();
    assert.equal(state.sessions.length, 1);
    assert.equal(state.sessions[0].thread_id, threadId);
    assert.equal(state.connections, 1);
    assert.equal(state.actions.length, 1, 'takeover does not create a second protocol session');
  });
  await test('thread protocol logs render compact highlighted JSON and expand without changing payloads', async t => {
    const { page, state } = await fixture(t);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).fill('Unsent');
    const params = { protocolVersion: 1, clientCapabilities: { fs: { readTextFile: true, writeTextFile: true } }, note: 'literal {x}: y, z\n<script>alert(1)</script>', long: 'command '.repeat(80) };
    const record = (index, direction, wire, shared = false) => ({ timestamp: `2026-10-11T00:00:0${index}Z`, direction, connection_id: 'connection', thread_id: shared ? null : 'local', session_id: shared ? null : 'remote', scope: shared ? 'connection' : 'thread', message: JSON.stringify(wire) });
    state.logs = [
      record(0, 'outgoing', { id: 'init-request', method: 'initialize', params }, true),
      record(1, 'incoming', { id: 'init-request', result: { protocolVersion: 1, agentInfo: { name: 'codex-acp' } } }, true),
      record(2, 'outgoing', { id: 0, method: 'session/new', params: { cwd: '/repo', mcpServers: [] } }),
      record(3, 'incoming', { method: '_auth/status_update', params: { authStatus: { kind: 'gateway', label: 'Custom model gateway', detail: 'openai_http' } } }),
      record(4, 'incoming', { id: 0, result: { sessionId: 'remote' } }),
    ];
    await page.evaluate(() => { navigator.clipboard.writeText = async value => { window.copiedLog = value; }; });
    await page.getByRole('button', { name: '会话更多操作', exact: true }).click();
    await page.getByRole('menuitem', { name: 'ACP 协议日志', exact: true }).click();
    const logs = page.getByRole('region', { name: 'ACP 协议日志', exact: true });
    const rows = logs.locator('.acp-log-row');
    await rows.nth(4).waitFor();
    assert.deepEqual(state.logReads, ['/api/acp/sessions/local/logs']);
    assert.deepEqual(await rows.locator('.acp-log-method').allTextContents(), ['initialize', 'initialize', 'session/new', '_auth/status_update', 'session/new']);
    assert.equal(await logs.locator('.acp-log-shared').count(), 2);
    assert.equal(await rows.nth(4).locator('.acp-log-id').textContent(), '0');
    assert.equal(await rows.nth(3).locator('.acp-log-badge').textContent(), 'Notification');
    const preview = rows.first().locator('.acp-log-preview');
    assert.equal((await preview.textContent()).includes('\n'), false);
    assert.deepEqual(JSON.parse(await preview.textContent()), params);
    assert.ok(await preview.locator('.token.property').count() > 0);
    const size = await preview.evaluate(element => ({ height: element.clientHeight, line: parseFloat(getComputedStyle(element).lineHeight), width: element.scrollWidth, viewport: element.parentElement.clientWidth }));
    assert.ok(size.height <= size.line + 1 && size.width > size.viewport, 'collapsed JSON stays one line and scrolls horizontally');
    assert.equal(await logs.locator('script,img').count(), 0, 'JSON strings are rendered as text');
    await preview.click({ position: { x: 20, y: 8 } });
    const expanded = rows.first().locator('pre');
    assert.equal(await expanded.textContent(), JSON.stringify(params, null, 2));
    await rows.first().getByRole('button', { name: 'Copy Message', exact: true }).click();
    assert.deepEqual(JSON.parse(await page.evaluate(() => window.copiedLog)), params);
    await rows.first().locator('.acp-log-heading').click();
    assert.equal(await rows.first().locator('pre').count(), 0);
    await rows.nth(3).locator('.acp-log-heading').focus();
    await page.keyboard.press('Enter');
    assert.match(await rows.nth(3).locator('pre').textContent(), /\n {2}"authStatus"/);
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-protocol-logs.png` });
    await rows.last().locator('.acp-log-heading').click();
    assert.deepEqual(JSON.parse(await rows.last().locator('pre').textContent()), { sessionId: 'remote' });
    await logs.getByRole('button', { name: 'Copy All Messages', exact: true }).click();
    const copied = JSON.parse(await page.evaluate(() => window.copiedLog));
    assert.equal(copied.length, 5); assert.equal(copied[4].method, 'session/new');
    assert.equal(copied[0].scope, 'connection'); assert.equal(copied[4].thread_id, 'local');
    state.logs.push(record(5, 'incoming', { method: 'session/update', params: { sessionId: 'remote', update: { sessionUpdate: 'available_commands_update', availableCommands: [] } } }));
    await logs.getByRole('button', { name: 'Refresh Messages', exact: true }).click();
    await rows.nth(5).waitFor();
    assert.equal(await rows.nth(3).locator('.acp-log-heading').getAttribute('aria-expanded'), 'true');
    await logs.getByRole('button', { name: 'Clear Messages', exact: true }).click();
    await logs.getByText('No messages recorded yet', { exact: true }).waitFor();
    state.logs.push(record(6, 'incoming', { method: 'session/update', params: { sessionId: 'remote' } }));
    await logs.getByRole('button', { name: 'Refresh Messages', exact: true }).click();
    await rows.first().waitFor(); assert.equal(await rows.count(), 1, 'clear affects the view and refresh only reveals newer messages');
    await page.setViewportSize({ width: 390, height: 844 });
    await page.locator('aside').evaluate(element => { element.style.display = 'none'; });
    assert.ok(await logs.evaluate(element => element.scrollWidth <= element.clientWidth), 'logs fit a narrow tab');
    await logs.getByRole('button', { name: '返回会话', exact: true }).click();
    assert.equal(await page.getByLabel('ACP 消息', { exact: true }).innerText(), 'Unsent');
  });
  await test('protocol log response names are isolated by connection, direction and typed request ID', async t => {
    const { page } = await fixture(t);
    const result = await page.evaluate(async () => {
      const { logMessages, payloadText } = await import('/src/features/acp/acp_tools/messages.ts');
      const wire = (connection, direction, message) => ({ connection_id: connection, direction, timestamp: '', message: JSON.stringify(message), thread_id: 'local', session_id: 'remote', scope: 'thread' });
      const entries = [
        wire('a', 'outgoing', { id: 1, method: 'session/new' }),
        wire('a', 'incoming', { id: 1, method: 'fs/read_text_file' }),
        wire('b', 'outgoing', { id: 1, method: 'session/load' }),
        wire('a', 'outgoing', { id: '1', method: 'session/prompt' }),
        wire('a', 'outgoing', { id: 1, result: { content: 'read' } }),
        wire('a', 'incoming', { id: '1', error: { code: -32603, message: 'failed' } }),
        wire('b', 'incoming', { id: 1, result: null }),
        wire('a', 'incoming', { id: 1, result: { sessionId: 'remote' } }),
        wire('a', 'incoming', { id: 'expired', result: false }),
        wire('a', 'incoming', [{ method: 'one', params: {} }, { method: 'two', params: [] }]),
        { ...wire('a', 'stderr', null), message: 'adapter stderr' },
        { ...wire('a', 'incoming', null), message: '{truncated' },
      ];
      return logMessages(entries).map(message => ({ name: message.name, type: message.type, failed: message.failed, body: payloadText(message, false) }));
    });
    assert.deepEqual(result.slice(4, 9).map(message => message.name), ['fs/read_text_file', 'session/prompt', 'session/load', 'session/new', '[response: request outside exported log]']);
    assert.equal(result[5].failed, true); assert.equal(result[6].body, 'null');
    assert.deepEqual(result.slice(9, 11).map(message => message.name), ['one', 'two']);
    assert.equal(result[11].body, 'adapter stderr'); assert.equal(result[12].body, '{truncated');
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
    assert.equal(await form.getByRole('radio', { name: 'Large', exact: true }).isChecked(), true);
    await form.getByRole('checkbox', { name: 'Read', exact: true }).check();
    await form.getByRole('button', { name: '确认', exact: true }).click();
    await form.getByRole('alert').getByText('Scopes：所选数量不符合要求。', { exact: true }).waitFor();
    assert.equal(interactions(state).length, 0);
    await form.getByRole('checkbox', { name: 'Write', exact: true }).check();
    await form.getByRole('button', { name: '确认', exact: true }).click();
    await form.waitFor({ state: 'detached' });
    assert.deepEqual(interactions(state)[0].response, { action: 'accept', content: { model: 'large', enabled: false, count: 2, scopes: ['read', 'write'] } });
  });
  await test('question and permission cards share the message width and retain explicit responses', async t => {
    const { page, state } = await fixture(t);
    await page.setViewportSize({ width: 1440, height: 960 });
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: '用 ask user question 来问我' } },
      { id: 'a', kind: 'assistant', content: { type: 'text', text: '需要确认一些信息。' } },
    ] });
    await newSession(page);
    const request = { mode: 'form', message: '你在哪个城市？我需要知道城市才能查询今天的天气。', requestedSchema: { type: 'object', properties: {
      city: { type: 'string', title: '所在城市', oneOf: [{ const: 'beijing', title: '北京' }, { const: 'shanghai', title: '上海', description: '查询当地天气和出行建议' }] },
      other: { type: 'string', title: 'Other', description: '也可以填写其他城市。', minLength: 2 },
    } } };
    state.session.permissions = [{ id: 'city', kind: 'elicitation', request }]; state.session.revision++;
    const form = page.getByRole('form', { name: 'Agent 信息请求', exact: true });
    await form.waitFor();
    const aligned = async card => {
      const box = await card.boundingBox();
      const message = await page.locator('.project-aow-snapshot-message.assistant .project-aow-snapshot-bubble').boundingBox();
      assert.ok(Math.abs(box.x - message.x) <= 1);
      assert.ok(Math.abs(box.width - message.width) <= 1);
      assert.equal(await card.evaluate(element => element.scrollWidth <= element.clientWidth + 1), true);
    };
    await aligned(form);
    assert.equal(await form.getByRole('radio', { checked: true }).count(), 0, 'request arrival never selects or submits an answer');
    await form.getByRole('radio', { name: '北京', exact: true }).focus();
    await page.keyboard.press('ArrowDown');
    assert.equal(await form.getByRole('radio', { name: /上海/ }).isChecked(), true);
    await form.getByLabel('Other', { exact: true }).fill('宁波');
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-question.png` });
    await page.addStyleTag({ content: '@media (max-width: 720px) { #root > div > div > aside { display: none; } }' });
    await page.setViewportSize({ width: 390, height: 900 });
    await aligned(form);
    const activity = await page.locator('.acp-activity').boundingBox();
    const submit = await form.getByRole('button', { name: '确认', exact: true }).boundingBox();
    assert.ok(submit.y >= activity.y && submit.y + submit.height <= activity.y + activity.height, 'submit remains visible while the form body scrolls');
    assert.equal(await form.locator('.acp-request-body').evaluate(element => element.scrollHeight > element.clientHeight), true);
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-question-mobile.png` });
    assert.equal(interactions(state).length, 0);
    await form.getByRole('button', { name: '确认', exact: true }).click();
    await form.waitFor({ state: 'detached' });
    assert.deepEqual(interactions(state).at(-1).response, { action: 'accept', content: { city: 'shanghai', other: '宁波' } });
    for (const [label, action] of [['拒绝', 'decline'], ['取消', 'cancel']]) {
      state.session.permissions = [{ id: action, kind: 'elicitation', request }]; state.session.revision++;
      await form.getByRole('button', { name: label, exact: true }).click();
      await form.waitFor({ state: 'detached' });
      assert.deepEqual(interactions(state).at(-1).response, { action });
    }
    await page.setViewportSize({ width: 1440, height: 960 });
    state.session.permissions = [{ id: 'permission', kind: 'permission', request: { toolCall: { title: '读取项目配置', rawInput: { command: 'cat README.md' } }, options: [
      { optionId: 'allow', name: '允许一次', kind: 'allow_once' }, { optionId: 'always', name: '始终允许', kind: 'allow_always' }, { optionId: 'deny', name: '拒绝', kind: 'reject_once' },
    ] } }]; state.session.revision++;
    const permission = page.getByRole('region', { name: 'Agent 权限请求', exact: true });
    await permission.waitFor();
    assert.equal(await permission.getByRole('button').count(), 3, 'only agent-provided permission options are offered');
    await aligned(permission);
    assert.equal(await permission.locator('pre').isVisible(), false);
    await permission.locator('summary').click();
    await permission.getByText(/cat README.md/).waitFor();
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-permission.png` });
    await page.setViewportSize({ width: 390, height: 900 });
    await aligned(permission);
    await permission.getByRole('button', { name: '拒绝', exact: true }).click();
    await permission.waitFor({ state: 'detached' });
    assert.deepEqual(interactions(state).at(-1).response, { outcome: { outcome: 'selected', optionId: 'deny' } });
  });
  await test('the host imports only the ACP facade and places ACP after Terminal', async () => {
    const source = await readFile(new URL('../src/aow/ProjectAow.tsx', import.meta.url), 'utf8');
    assert.ok(source.includes("from '../features/acp'"));
    assert.ok(!source.includes("from '../features/acp/"));
    const navigation = source.slice(source.indexOf('<nav aria-label="AoW side views">'));
    assert.ok(navigation.indexOf('aria-label="Terminal 面板"') < navigation.indexOf('aria-label="ACP 面板"'));
    assert.ok(navigation.indexOf('aria-label="ACP 面板"') < navigation.indexOf('aria-label="Conversation"'));
  });
  await test('Upstream config providers replace legacy modes and selecting a value persists its default', async t => {
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
    const composer = await page.locator('.acp-composer').boundingBox();
    const options = await page.getByLabel('会话配置', { exact: true }).boundingBox();
    assert.ok(options.y > composer.y && options.y + options.height <= composer.y + composer.height);
    await page.getByRole('button', { name: '移动会话视图', exact: true }).evaluate(button => { button.style.display = 'none'; });
    await page.screenshot({ path: '/tmp/aow-acp-aligned-conversation.png' });
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
  await test('rendering follows Upstream visibility, streamed thinking and tool permission rules', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: '**literal user input**' } },
      { id: 'empty', kind: 'assistant', content: { type: 'text', text: '   ' } },
      { id: 'thought', kind: 'thought', content: { type: 'text', text: 'Consider the request carefully.' } },
    ] });
    await newSession(page);
    const thought = page.getByRole('button', { name: '思考过程', exact: true });
    await toggleThinking(page);
    await page.waitForFunction(() => document.querySelector('[aria-label="思考过程"]')?.getAttribute('aria-expanded') === 'true');
    assert.equal(await page.locator('.acp-user-text').innerText(), '**literal user input**');
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
    const actions = page.locator('.acp-message-actions');
    assert.equal(await actions.count(), 1, 'only the previous completed turn has controls');
    assert.equal(await page.getByTitle('Scroll to Top', { exact: true }).count(), 0, 'historical controls have no top action');
    state.session.entries.push({ id: 'command', kind: 'tool', content: { title: 'Read files', kind: 'execute', status: 'completed', content: [] } }, entry('a3', 'assistant', 'Final answer'));
    state.session.revision++;
    await page.getByText('Final answer', { exact: true }).waitFor();
    assert.equal(await actions.count(), 1, 'streamed final text is still generating');
    state.session.status = 'idle'; state.session.revision++;
    await page.waitForFunction(() => document.querySelectorAll('.acp-message-actions').length === 2);
    assert.equal(await page.locator('[data-entry-id="a2"] .acp-message-actions').count(), 0);
    assert.deepEqual(await actions.first().getByRole('button').evaluateAll(buttons => buttons.map(button => button.title)), ['Copy This Agent Response', 'Scroll to User Message']);
    assert.deepEqual(await actions.last().getByRole('button').evaluateAll(buttons => buttons.map(button => button.title)), ['Copy This Agent Response', 'Scroll to User Message', 'Scroll to Top']);
    await page.evaluate(() => {
      navigator.clipboard.writeText = async value => { window.copiedReply = value; };
      Element.prototype.scrollIntoView = function () { window.scrolledEntry = this.getAttribute('data-entry-id'); };
    });
    await actions.last().getByRole('button', { name: '复制回复', exact: true }).click();
    assert.equal(await page.evaluate(() => window.copiedReply), 'Final answer');
    await actions.last().getByRole('button', { name: '跳到用户消息', exact: true }).click();
    assert.equal(await page.evaluate(() => window.scrolledEntry), 'u2', 'skip intervening tool and commentary');
    state.session.permissions = [{ id: 'pending', kind: 'permission', request: { toolCall: { title: 'Confirmation' }, options: [] } }]; state.session.revision++;
    await page.getByRole('region', { name: 'Agent 权限请求', exact: true }).waitFor();
    await page.waitForFunction(() => document.querySelectorAll('.acp-message-actions').length === 1);
    assert.equal(await page.getByTitle('Scroll to Top', { exact: true }).count(), 0, 'pending confirmation hides bottom navigation');
    state.session.permissions = []; state.session.status = 'working';
    state.session.entries.push(entry('u3', 'user', 'Third prompt'), entry('a4', 'assistant', 'Next response')); state.session.revision++;
    await page.getByText('Next response', { exact: true }).waitFor();
    assert.equal(await actions.count(), 2, 'historical turn controls remain while the next turn runs');
    state.session.status = 'idle'; state.session.stop_reason = 'cancelled'; state.session.revision++;
    await page.waitForFunction(() => document.querySelectorAll('.acp-message-actions').length === 3);
    assert.equal(await page.getByTitle('Scroll to Top', { exact: true }).count(), 1, 'only the latest completed turn offers top navigation');
  });
  await test('reply footer navigates to its user and conversation top without moving the composer or losing drafts', async t => {
    const { page, state } = await fixture(t);
    await page.setViewportSize({ width: 1440, height: 900 });
    state.session.entries = Array.from({ length: 3 }, (_, index) => [
      { id: `u${index}`, kind: 'user', content: { type: 'text', text: `Prompt ${index}` } },
      { id: `a${index}`, kind: 'assistant', content: { type: 'text', text: `Answer ${index}\n\n${'Long response paragraph.\n\n'.repeat(25)}` } },
    ]).flat();
    await newSession(page);
    const input = page.getByLabel('ACP 消息', { exact: true });
    await input.fill('Unsent draft');
    const fixedPositions = async () => ({ header: await page.locator('.project-aow-snapshot-header').boundingBox(), composer: await page.locator('.acp-composer').boundingBox() });
    const before = await fixedPositions();
    const latest = page.locator('.acp-message-actions').last();
    await latest.getByTitle('Scroll to User Message', { exact: true }).click();
    await page.waitForFunction(() => {
      const conversation = document.querySelector('.acp-conversation');
      return conversation.scrollTop > 100 && Math.abs(document.querySelector('[data-entry-id="u2"]').getBoundingClientRect().top - conversation.getBoundingClientRect().top) < 2;
    });
    await latest.getByTitle('Scroll to Top', { exact: true }).click();
    await page.waitForFunction(() => document.querySelector('.acp-conversation').scrollTop === 0 && document.querySelector('.project-aow-snapshot-navigation button')?.getAttribute('aria-current') === 'location');
    state.session.entries.at(-1).content.text += '\n\nUpdated response'; state.session.revision++;
    await page.waitForFunction(() => document.querySelector('[data-entry-id="a2"]').textContent.includes('Updated response'));
    assert.equal(await page.locator('.acp-conversation').evaluate(element => element.scrollTop), 0, 'polling preserves the top navigation position');
    assert.deepEqual(await fixedPositions(), before);
    assert.equal(await input.innerText(), 'Unsent draft');
  });
  await test('tool-only completed turns keep navigation without a copy action and handle missing user history', async t => {
    const { page, state } = await fixture(t);
    state.session.entries = [{ id: 'tool', kind: 'tool', content: { title: 'Read files', status: 'completed', rawOutput: 'Tool output' } }];
    await newSession(page);
    const actions = page.locator('.acp-message-actions');
    await actions.waitFor();
    assert.deepEqual(await actions.getByRole('button').evaluateAll(buttons => buttons.map(button => button.title)), ['Scroll to User Message', 'Scroll to Top']);
    await page.evaluate(() => {
      document.querySelector('.acp-conversation').scrollTo = options => { window.footerScrollTarget = options.top; };
    });
    await actions.getByTitle('Scroll to User Message', { exact: true }).click();
    assert.equal(await page.evaluate(() => window.footerScrollTarget), await page.locator('.acp-conversation').evaluate(element => element.scrollHeight), 'missing user history falls back to the bottom');
    await actions.getByTitle('Scroll to Top', { exact: true }).click();
    assert.equal(await page.evaluate(() => window.footerScrollTarget), 0);
  });
  await test('tool endings retain turn navigation without mislabeling progress as a final reply', async t => {
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
    assert.equal(await page.locator('.acp-message-actions').count(), 0);
    state.session.status = 'idle'; state.session.revision++;
    const actions = page.locator('.acp-message-actions');
    await actions.waitFor();
    assert.equal(await page.locator('.acp-turn-response > .acp-content .acp-message-actions').count(), 1);
    await page.evaluate(() => {
      navigator.clipboard.writeText = async value => { window.copiedReply = value; };
      Element.prototype.scrollIntoView = function () { window.scrolledEntry = this.getAttribute('data-entry-id'); };
    });
    assert.equal(await actions.getByRole('button', { name: '复制回复', exact: true }).count(), 0);
    assert.equal(await page.getByText('Checking README', { exact: true }).isVisible(), false);
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
    await tool.locator('.acp-diff .modified-in-monaco-diff-editor').waitFor();
    assert.match(await tool.locator('.acp-diff .modified-in-monaco-diff-editor').textContent(), /const\s+after\s*=\s*2/);
    assert.equal(await tool.getByRole('button', { name: 'Review file changes', exact: true }).count(), 0, 'permission previews have no collapse control');
    assert.equal(await page.locator('.acp-message-actions').count(), 0);
    state.session.entries[0].content = { ...content, status: 'cancelled', content: [] }; state.session.revision++;
    await tool.locator('.acp-diff').waitFor({ state: 'detached' });
    await tool.getByRole('button', { name: 'Allow once', exact: true }).click();
    assert.equal(state.actions.at(-1).request_id, 'permission');
  });
  await test('distinct protocol messages render on separate lines through streaming and reload', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Run background task' } },
      { id: 'main', message_id: 'msg_119', kind: 'assistant', content: { type: 'text', text: 'ACP_LIVE_MAIN_DONE' } },
    ] });
    await newSession(page);
    const chunks = page.locator('.acp-message-chunk');
    await chunks.filter({ hasText: 'ACP_LIVE_MAIN_DONE' }).waitFor();
    state.session.entries.push({ id: 'followup', message_id: 'msg_122', kind: 'assistant', content: { type: 'text', text: 'ACP_L' } });
    state.session.revision++;
    await chunks.getByText('ACP_L', { exact: true }).waitFor();
    state.session.entries.at(-1).content.text = 'ACP_LIVE_FOLLOWUP_VISIBLE';
    state.session.status = 'idle'; state.session.revision++;
    const expected = ['ACP_LIVE_MAIN_DONE', 'ACP_LIVE_FOLLOWUP_VISIBLE'];
    const checkLines = async () => {
      await chunks.filter({ hasText: expected[1] }).waitFor();
      assert.equal(await chunks.filter({ hasText: expected[0] }).isVisible(), false, 'earlier messages are folded into the process');
      await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
      assert.deepEqual((await chunks.allTextContents()).map(text => text.trim()), expected);
      const main = await chunks.nth(0).boundingBox();
      const followup = await chunks.nth(1).boundingBox();
      assert.ok(followup.y >= main.y + main.height, 'follow-up starts below the main reply');
    };
    await checkLines();
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await checkLines();
  });
  await test('out-of-band plans and notices preserve streamed Markdown and thought expansion', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Inspect' } },
      { id: 'thought', kind: 'thought', content: { type: 'text', text: 'Thinking about files' } },
    ] });
    await newSession(page);
    const thinking = page.getByRole('button', { name: '思考过程', exact: true });
    await toggleThinking(page);
    await page.getByText('Thinking about files', { exact: true }).waitFor();
    state.session.plan = { entries: [{ content: 'Read file', status: 'in_progress' }] };
    state.session.notices = [{ id: 'n', kind: 'notice', content: { severity: 'warning', title: 'Rate limit', description: 'Please **wait** before retrying.' } }];
    state.session.revision++;
    const notice = page.getByRole('alert', { name: 'Rate limit', exact: true });
    await notice.waitFor();
    assert.equal(await notice.getByText('Please **wait** before retrying.', { exact: true }).isVisible(), false);
    await notice.getByRole('button', { name: '详情', exact: true }).click();
    await notice.getByText('Please **wait** before retrying.', { exact: true }).waitFor();
    assert.equal(await thinking.getAttribute('aria-expanded'), 'true');
    assert.equal(await notice.getAttribute('data-severity'), 'warning');
    state.session.entries.push({ id: 'a', kind: 'assistant', content: { type: 'text', text: '**Hel' } }); state.session.revision++;
    await page.getByText('**Hel', { exact: true }).waitFor();
    state.session.entries.at(-1).content.text += 'lo**';
    state.session.plan.entries[0].status = 'completed'; state.session.revision++;
    await page.locator('.acp-message-chunk strong').filter({ hasText: /^Hello$/ }).waitFor();
    assert.equal(await page.locator('.acp-message-chunk').count(), 1);
    await page.getByText('All Done', { exact: true }).waitFor();
    assert.equal(await notice.getByRole('button', { name: '收起', exact: true }).getAttribute('aria-expanded'), 'true', 'stream updates preserve expanded notice details');
    await notice.getByRole('button', { name: '关闭通知：Rate limit', exact: true }).click();
    await notice.waitFor({ state: 'detached' });
    assert.deepEqual(state.actions.at(-1), { action: 'dismiss_notice', notice_id: 'n' });
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await page.locator('.acp-message-chunk strong').filter({ hasText: /^Hello$/ }).waitFor();
    assert.equal(await notice.count(), 0);
  });
  await test('notices align with the conversation and keep long details readable on narrow screens', async t => {
    const { page, state } = await fixture(t);
    const fallback = 'Falling back from WebSockets to HTTPS transport. workspace routing discovery timed out';
    const longTitle = '  Connection diagnostics:\n' + 'workspace-routing-discovery/'.repeat(15) + '\n  ';
    const description = '  Agent connection failed.\n' + '<script>unsafe()</script>\n'.repeat(30) + '  ';
    Object.assign(state.session, { entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Check the workspace connection' } },
      { id: 'a', kind: 'assistant', content: { type: 'text', text: 'The connection is being checked.' } },
    ], notices: [
      { id: 'transport', kind: 'notice', content: { severity: 'warning', title: fallback } },
      { id: 'error', kind: 'notice', content: { severity: 'error', title: '无法继续执行任务', description } },
      { id: 'info', kind: 'notice', content: { message: longTitle } },
    ] });
    await page.addStyleTag({ content: '@media (max-width: 720px) { #root > div > div > aside { display: none; } }' });
    await newSession(page);
    const warning = page.getByRole('alert', { name: fallback, exact: true });
    const error = page.getByRole('alert', { name: '无法继续执行任务', exact: true });
    const info = page.locator('.acp-notice[data-severity="info"]');
    await warning.waitFor();
    assert.equal(await warning.locator('.acp-notice-title').textContent(), fallback, 'transport notices preserve the original backend text');
    assert.equal(await info.locator('.acp-notice-title').textContent(), longTitle, 'long titles preserve every line and whitespace');
    assert.equal(await warning.getByRole('button', { name: '详情', exact: true }).count(), 0, 'titles alone do not create synthetic details');
    assert.equal(await info.getByRole('button', { name: '详情', exact: true }).count(), 0);
    const aligned = async () => {
      const composer = await page.locator('.acp-composer').boundingBox();
      const message = await page.locator('.project-aow-snapshot-message.assistant .project-aow-snapshot-bubble').boundingBox();
      for (const notice of [warning, error, info]) {
        const box = await notice.boundingBox();
        assert.ok(Math.abs(box.x - composer.x) <= 1 && Math.abs(box.width - composer.width) <= 1, 'notice aligns with the composer');
        assert.ok(Math.abs(box.x - message.x) <= 1 && Math.abs(box.width - message.width) <= 1, 'notice aligns with the conversation');
        assert.equal(await notice.evaluate(element => element.scrollWidth <= element.clientWidth + 1), true, 'long text does not cause horizontal overflow');
        const close = await notice.getByRole('button', { name: /^关闭通知/ }).boundingBox();
        assert.ok(close.x >= box.x && close.x + close.width <= box.x + box.width, 'close stays inside the notice');
      }
    };
    for (const width of [1680, 760, 390]) {
      await page.setViewportSize({ width, height: 1000 });
      await aligned();
      if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-notices-${width}.png` });
    }
    await error.getByRole('button', { name: '详情', exact: true }).focus();
    await page.keyboard.press('Enter');
    const toggle = error.getByRole('button', { name: '收起', exact: true });
    assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
    assert.equal(await toggle.getAttribute('aria-controls'), await error.locator('.acp-notice-description').getAttribute('id'));
    await toggle.click();
    await error.getByRole('button', { name: '详情', exact: true }).click();
    const details = error.locator('.acp-notice-description');
    assert.equal(await details.textContent(), description, 'description text matches backend logs without trimming or rewriting');
    assert.equal(await details.evaluate(element => element.scrollHeight > element.clientHeight), true, 'large details scroll within the notice');
    assert.equal(await details.locator('script').count(), 0, 'details remain plain text');
    await aligned();
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-notices-expanded.png` });
    await error.getByRole('button', { name: '关闭通知：无法继续执行任务', exact: true }).click();
    await error.waitFor({ state: 'detached' });
    assert.deepEqual(state.actions.at(-1), { action: 'dismiss_notice', notice_id: 'error' });
    assert.equal(await warning.isVisible(), true);
  });
  await test('plan cards align with messages and preserve collapse through updates and persistent clearing', async t => {
    const { page, state } = await fixture(t);
    await page.setViewportSize({ width: 1680, height: 1000 });
    Object.assign(state.session, { status: 'working', entries: [
      { id: 'u', kind: 'user', content: { type: 'text', text: 'Inspect the project' } },
      { id: 'a', kind: 'assistant', content: { type: 'text', text: 'I will inspect the project.' } },
    ] });
    await newSession(page);
    state.session.plan = { entries: [
      { content: 'Read **project configuration**', status: 'in_progress' },
      { content: 'Review `src/main.rs`', status: 'pending' },
    ] }; state.session.revision++;
    const card = page.getByRole('region', { name: 'Plan', exact: true });
    await card.getByRole('button', { name: '展开计划', exact: true }).waitFor();
    assert.equal(await card.locator('ol').isVisible(), false, 'plans start collapsed like Zed');
    await card.getByText('Current:', { exact: true }).waitFor();
    await card.getByRole('button', { name: '展开计划', exact: true }).click();
    await card.getByText('2 Tasks', { exact: true }).waitFor();
    assert.equal(await card.locator('li strong').innerText(), 'project configuration');
    const aligned = async () => {
      const plan = await card.boundingBox();
      const message = await page.locator('.project-aow-snapshot-message.assistant .project-aow-snapshot-bubble').boundingBox();
      assert.ok(Math.abs(plan.x - message.x) <= 1, `plan and message left edges: ${plan.x}, ${message.x}`);
      assert.ok(Math.abs(plan.width - message.width) <= 1, `plan and message widths: ${plan.width}, ${message.width}`);
      assert.ok(plan.width <= 900);
    };
    await aligned();
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-plan-expanded.png` });
    await card.getByRole('button', { name: '收起计划', exact: true }).click();
    assert.equal(await card.locator('ol').isVisible(), false);
    await card.getByText('Current:', { exact: true }).waitFor();
    await card.getByText('2 left', { exact: true }).waitFor();
    state.session.plan.entries[0].status = 'completed';
    state.session.plan.entries[1].status = 'in_progress'; state.session.revision++;
    await card.locator('.acp-plan-current code').getByText('src/main.rs', { exact: true }).waitFor();
    assert.equal(await card.getByRole('button', { name: '展开计划', exact: true }).getAttribute('aria-expanded'), 'false');
    await card.getByText('1 left', { exact: true }).waitFor();
    await card.getByRole('button', { name: '展开计划', exact: true }).click();
    await card.getByText('1/2', { exact: true }).waitFor();
    const completed = await card.locator('li[data-status="completed"]').evaluate(element => ({ color: getComputedStyle(element.querySelector('svg')).color, strike: getComputedStyle(element.querySelector('.acp-markdown')).textDecorationLine }));
    assert.equal(completed.strike, 'line-through');
    assert.equal(completed.color, 'rgb(78, 201, 176)');
    state.session.plan.entries[1].status = 'cancelled'; state.session.revision++;
    await card.getByText('Finished', { exact: true }).waitFor();
    state.session.plan.entries[0].status = 'cancelled'; state.session.revision++;
    await card.getByText('Cancelled', { exact: true }).waitFor();
    state.session.plan.entries = Array.from({ length: 20 }, (_, index) => ({ content: `Task ${index}: ${'long content '.repeat(20)}`, status: 'pending' })); state.session.revision++;
    await card.getByText('20 Tasks', { exact: true }).waitFor();
    assert.ok((await card.locator('ol').boundingBox()).height <= 160);
    assert.equal(await card.locator('ol').evaluate(element => element.scrollHeight > element.clientHeight), true);
    state.session.entries[1].content.text += `\n\n${'A longer streamed response.\n\n'.repeat(40)}`; state.session.revision++;
    await page.getByText('A longer streamed response.', { exact: true }).first().waitFor();
    await page.addStyleTag({ content: '@media (max-width: 720px) { #root > div > div > aside { display: none; } }' });
    for (const width of [1000, 720, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await aligned();
      const dimensions = await card.evaluate(element => ({ scroll: element.scrollWidth, client: element.clientWidth }));
      assert.ok(dimensions.scroll <= dimensions.client + 1, `plan overflow at ${width}px: ${JSON.stringify(dimensions)}`);
    }
    if (process.env.ACP_SCREENSHOT_DIR) await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-plan-mobile.png` });
    await card.getByRole('button', { name: '清除计划', exact: true }).click();
    await card.waitFor({ state: 'detached' });
    assert.deepEqual(state.actions.at(-1), { action: 'clear_plan' });
    await page.setViewportSize({ width: 1680, height: 1000 });
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await page.getByText('I will inspect the project.', { exact: true }).waitFor();
    assert.equal(await card.count(), 0, 'cleared plans do not return on polling or reload');
    state.session.plan = { entries: [{ content: 'New task', status: 'pending' }] }; state.session.revision++;
    await card.getByRole('button', { name: '展开计划', exact: true }).click();
    await card.getByText('New task', { exact: true }).waitFor();
  });
  await test('compaction cards follow lifecycle and expose only available details', async t => {
    const { page, state } = await fixture(t);
    const content = { compactionId: 'c', status: 'in_progress', summary: [] };
    Object.assign(state.session, { status: 'working', entries: [{ id: 'c', kind: 'compaction_update', content }] });
    await newSession(page);
    await page.getByText('上下文压缩中…', { exact: true }).waitFor();
    assert.equal(await page.locator('details.acp-compaction').count(), 0, 'empty state has no expansion');
    content.summary = [{ type: 'text', text: '**Retained summary**' }]; state.session.revision++;
    const card = page.locator('details.acp-compaction');
    await card.locator('summary').click();
    await card.locator('strong').filter({ hasText: 'Retained summary' }).waitFor();
    content.status = 'failed'; content.error = 'Context limit reached'; state.session.revision++;
    await card.getByText('上下文压缩失败', { exact: true }).waitFor();
    await card.getByRole('alert').filter({ hasText: 'Context limit reached' }).waitFor();
    assert.equal(await card.getAttribute('open'), '');
    assert.equal(await page.locator('.acp-compaction').count(), 1);
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
      const link = page.getByRole('link', { name: label, exact: true });
      const destination = new URL(await link.getAttribute('href'), page.url());
      const separator = expected.lastIndexOf(':');
      assert.equal(destination.pathname, '/aow/tabs/file');
      assert.equal(destination.searchParams.get('workspace'), 'fixture-workspace');
      assert.equal(destination.searchParams.get('path'), expected.slice(0, separator));
      assert.equal(destination.hash, '#L' + expected.slice(separator + 1));
      await link.click();
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
      document.execCommand = (...args) => { window.copiedReply = document.querySelector('.acp-clipboard-input')?.value; return original(...args); };
    });
    const copy = page.getByRole('button', { name: '复制回复', exact: true });
    await copy.click();
    await page.waitForFunction(() => window.copiedReply === 'LAN reply');
    assert.equal(await page.evaluate(() => window.copiedReply), 'LAN reply');
    assert.equal(await page.locator('.acp-clipboard-input').count(), 0);
    await page.evaluate(() => {
      Object.defineProperty(navigator, 'clipboard', { value: { writeText: async () => { throw new Error('denied'); } }, configurable: true });
      document.execCommand = () => false;
    });
    await copy.click();
    await page.getByRole('alert').filter({ hasText: '复制失败，请选择文本后手动复制。' }).waitFor();
    assert.equal(await page.locator('.acp-clipboard-input').count(), 0);
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
    await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
    await page.getByRole('button', { name: '展开工具执行：2 次调用', exact: true }).click();
    assert.equal(await tool.locator('.xterm').count(), 0, 'completed process disposal resets terminal views');
    await tool.getByRole('button', { name: 'pwd', exact: true }).click();
    await tool.locator('.acp-terminal-output[data-exit-code="0"]').waitFor();
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
    await page.getByRole('button', { name: '展开处理过程', exact: true }).click();
    await page.getByRole('button', { name: '展开工具执行：2 次调用', exact: true }).click();
    await tool.getByRole('button', { name: 'pwd', exact: true }).click();
    await tool.getByText('first line', { exact: true }).waitFor();
    await tool.getByText('second line', { exact: true }).waitFor();
    await tool.locator('.acp-terminal-output[data-exit-code="0"]').waitFor();
  });
  await test('tool commands highlight Shell syntax, wrap literally and copy on LAN HTTP', async t => {
    const { page, state } = await fixture(t, false, { insecure: true });
    const command = 'printf "<tag>`literal`</tag>" && git status --short && rg --files ' + 'src/nested/path/'.repeat(25) + '\nprintf "$TASK_NAME" # Finished';
    Object.assign(state.session, { entries: [
      { id: 'a', kind: 'assistant', content: { type: 'text', text: 'Normal reply\nSecond line' } },
      { id: 'cmd', kind: 'tool', content: { title: command, kind: 'execute', status: 'completed', rawOutput: 'Command result' } },
    ] });
    await newSession(page);
    const tool = page.getByRole('region', { name: command, exact: true });
    const code = tool.locator('.acp-command-code > code');
    await code.waitFor();
    assert.equal(await code.textContent(), command);
    assert.equal(await code.locator('tag').count(), 0, 'commands are literal text');
    for (const token of ['builtin', 'function', 'parameter', 'string', 'variable', 'comment', 'operator']) {
      const highlighted = code.locator(`.token.${token}`).first();
      assert.equal(await highlighted.isVisible(), true, `${token} is highlighted`);
      assert.notEqual(await highlighted.evaluate(element => getComputedStyle(element).color), await code.evaluate(element => getComputedStyle(element).color));
    }
    await page.evaluate(() => {
      document.execCommand = action => { window.copiedCommand = document.querySelector('.acp-clipboard-input')?.value; return action === 'copy'; };
    });
    await tool.getByRole('button', { name: '复制命令', exact: true }).click();
    assert.equal(await page.evaluate(() => window.copiedCommand), command);
    const metrics = await page.evaluate(() => {
      const comparison = document.createElement('div');
      comparison.className = 'project-aow-session-snapshot aow-conversation-theme';
      comparison.innerHTML = '<div class="project-aow-snapshot-markdown conversation-prose">Session detail</div>';
      document.body.append(comparison);
      const style = selector => { const s = getComputedStyle(document.querySelector(selector)); return { fontSize: s.fontSize, lineHeight: s.lineHeight, fontFamily: s.fontFamily }; };
      const result = { reference: style('.project-aow-session-snapshot .project-aow-snapshot-markdown'), body: style('.acp-entry-assistant .project-aow-snapshot-markdown'), code: style('.acp-command-code'), meta: style('.acp-terminal-metadata') };
      comparison.remove(); return result;
    });
    assert.deepEqual(metrics.body, metrics.reference);
    assert.equal(metrics.body.fontSize, '13px');
    assert.equal(metrics.code.fontSize, '12px');
    assert.equal(metrics.meta.fontSize, '11px');
    assert.match(metrics.code.fontFamily, /monospace/);
    assert.equal(await page.locator('.acp-entry-assistant br').count(), 1, 'Agent Markdown preserves soft breaks');
    await page.setViewportSize({ width: 750, height: 850 });
    const width = await code.evaluate(element => ({ width: element.parentElement.clientWidth, scroll: element.parentElement.scrollWidth, height: element.getBoundingClientRect().height }));
    assert.ok(width.scroll <= width.width + 1, 'long commands wrap inside narrow cards');
    assert.ok(width.height > 34);
    await tool.getByRole('button', { name: command, exact: true }).click();
    await tool.getByText('Command result', { exact: true }).waitFor();
    assert.equal(await tool.getByText('Raw Input:', { exact: true }).count(), 0);
    assert.deepEqual(interactions(state), [], 'view and copy actions never execute commands');
    const large = 'printf "' + 'x'.repeat(33_000) + '"';
    state.session.entries[1].content.title = large; state.session.revision++;
    const updated = page.locator('.acp-command-code > code');
    await page.waitForFunction(title => document.querySelector('.acp-command-code > code')?.textContent === title, large);
    assert.equal(await updated.locator('.token').count(), 0, 'oversized commands retain their literal text without tokenization');
    const changed = 'git diff --stat\nprintf "<img src=x onerror=alert(1)>"';
    state.session.entries[1].content.title = changed; state.session.revision++;
    await page.waitForFunction(title => document.querySelector('.acp-command-code > code')?.textContent === title, changed);
    assert.equal(await updated.locator('.token.function').first().textContent(), 'git', 'command updates replace the highlight');
    assert.equal(await updated.locator('img,script,a').count(), 0, 'highlighting never interprets shell text as markup');
  });
  await test('tool details preserve Markdown labels, file navigation and input/output sections', async t => {
    const { page, state } = await fixture(t);
    const title = 'Read `src/main.rs`\nIgnored title continuation';
    state.session.entries = [{ id: 'read', kind: 'tool', content: {
      title, kind: 'read', status: 'completed', locations: [{ path: 'src/main.rs', line: 9 }], rawInput: { path: 'src/main.rs' },
      content: [{ type: 'content', content: { type: 'text', text: '```rust\nfn main() {}\n```' } }, { type: 'content', content: { type: 'resource_link', uri: 'file:///repo/README.md#L4' } }],
    } }];
    await newSession(page);
    const tool = page.getByRole('region', { name: title, exact: true });
    await tool.locator('.acp-tool-label code').waitFor();
    assert.equal(await tool.locator('.acp-command-code').count(), 0, 'ordinary tool labels are not treated as Shell');
    assert.equal(await tool.locator('.acp-tool-label code').textContent(), 'src/main.rs');
    assert.equal(await tool.locator('.acp-tool-label .lucide-search').count(), 1, 'completed tools keep their type icon');
    assert.equal(await tool.locator('.acp-tool-output').count(), 0);
    await tool.getByRole('button', { name: 'src/main.rs:9', exact: true }).click();
    assert.equal(await page.getByLabel('Opened file').textContent(), '/repo/src/main.rs:9');
    assert.equal(await tool.locator('.acp-tool-output').count(), 0, 'file navigation is independent of expansion');
    await tool.getByRole('button', { name: title, exact: true }).click();
    await tool.getByText('Raw Input:', { exact: true }).waitFor();
    await tool.getByText('Output:', { exact: true }).waitFor();
    assert.equal(await tool.locator('.acp-tool-input pre > code').textContent(), '{\n  "path": "src/main.rs"\n}\n');
    const output = tool.locator('.acp-tool-content').first();
    await output.getByRole('button', { name: '代码自动换行', exact: true }).click();
    assert.equal(await output.locator('pre').evaluate(element => getComputedStyle(element).whiteSpace), 'pre-wrap');
    await tool.getByRole('button', { name: '/repo/README.md#L4', exact: true }).click();
    assert.equal(await page.getByLabel('Opened file').textContent(), '/repo/README.md:4');
    state.session.entries[0].content.content[0].content.text = '```rust\nfn updated() {}\n```'; state.session.revision++;
    await output.getByText('fn updated() {}', { exact: true }).waitFor();
    assert.equal(await output.getByRole('button', { name: '复制代码', exact: true }).count(), 1);
    assert.equal(await output.locator('pre').evaluate(element => getComputedStyle(element).whiteSpace), 'pre-wrap', 'streaming preserves the wrap preference');
    await tool.getByRole('button', { name: '收起工具详情', exact: true }).click();
    assert.equal(await tool.locator('.acp-tool-output').count(), 0);
    if (process.env.ACP_SCREENSHOT_DIR) {
      await tool.getByRole('button', { name: title, exact: true }).click();
      await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-tool-details.png` });
    }
  });
  await test('tool permission previews put optional raw input after content without duplicated titles', async t => {
    const { page, state } = await fixture(t);
    const content = { toolCallId: 'search', title: 'Search project', kind: 'search', status: 'pending', rawInput: { query: 'needle' }, content: [{ type: 'content', content: { type: 'text', text: 'Review this search' } }] };
    state.session.entries = [{ id: 'search', kind: 'tool', content }];
    const options = [
      { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
      { optionId: 'always', name: "Yes, and don't ask again for commands that start with `git diff --cached --name-status`", kind: 'allow_always' },
      { optionId: 'deny', name: 'No, and tell TRAE CLI what to do differently', kind: 'reject_once' },
    ];
    state.session.permissions = [{ id: 'permission', kind: 'permission', request: { toolCall: content, options } }];
    await newSession(page);
    const tool = page.getByRole('region', { name: 'Search project', exact: true });
    await tool.getByText('Review this search', { exact: true }).waitFor();
    assert.equal(await tool.getByText('Search project', { exact: true }).count(), 1);
    assert.equal(await tool.getByRole('button', { name: 'Search project', exact: true }).count(), 0);
    assert.equal(await tool.locator('pre').count(), 0);
    await tool.getByRole('button', { name: 'View Raw Input', exact: true }).click();
    assert.match(await tool.locator('.acp-tool-confirm-input pre').textContent(), /needle/);
    assert.equal(await tool.locator('.acp-tool-permission pre').count(), 0);
    const permission = tool.getByRole('region', { name: 'Agent 权限请求', exact: true });
    assert.deepEqual(await permission.getByRole('button').allTextContents(), options.map(option => option.name), 'no synthetic Cancel choice is appended');
    await page.addStyleTag({ content: '@media (max-width: 720px) { #root > div > div > aside { display: none; } }' });
    for (const width of [1440, 720, 390]) {
      await page.setViewportSize({ width, height: 960 });
      const rows = await permission.getByRole('button').evaluateAll(buttons => buttons.map(button => {
        const rect = button.getBoundingClientRect();
        return { x: rect.x, y: rect.y, width: rect.width, bottom: rect.bottom, fits: button.scrollWidth <= button.clientWidth + 1 };
      }));
      assert.ok(rows.every((row, index) => row.fits && Math.abs(row.x - rows[0].x) < 1 && Math.abs(row.width - rows[0].width) < 1
        && (!index || row.y >= rows[index - 1].bottom)), `permission choices remain separate rows without horizontal overflow at ${width}px`);
    }
    await permission.getByRole('button', { name: options[0].name, exact: true }).focus();
    await page.keyboard.press('Enter');
    await tool.locator('.acp-tool-permission').waitFor({ state: 'detached' });
    assert.equal(await tool.locator('.acp-tool-output').count(), 0);
    assert.equal(state.actions.at(-1).request_id, 'permission');
    assert.deepEqual(state.actions.at(-1).response, { outcome: { outcome: 'selected', optionId: 'allow' } });
  });
  await test('terminal tool headers separate cwd, literal commands and failure from ANSI output', async t => {
    const { page, state } = await fixture(t);
    Object.assign(state.session, { terminals: { term: { cwd: '/repo/src', output: '\u001b[32mfirst line\u001b[0m\r\n' } }, entries: [
      { id: 'a', kind: 'assistant', content: { type: 'text', text: 'Checking the project.' } },
      { id: 'terminal', kind: 'tool', content: { title: 'cargo check --workspace', kind: 'execute', status: 'in_progress', content: [{ type: 'terminal', terminalId: 'term' }] } },
    ] });
    await newSession(page);
    const tool = page.getByRole('region', { name: 'cargo check --workspace', exact: true });
    await tool.locator('.acp-terminal-directory').waitFor();
    assert.equal(await tool.locator('.acp-terminal-directory').textContent(), '/repo/src');
    assert.equal(await tool.locator('.acp-command-code').textContent(), 'cargo check --workspace');
    assert.equal(await tool.locator('.acp-command-code .token.function').textContent(), 'cargo');
    assert.equal(await tool.locator('.acp-command-code .token.parameter').textContent(), '--workspace');
    assert.equal(await tool.getByLabel('执行中').count(), 1);
    assert.equal(await tool.locator('.xterm').count(), 0);
    await tool.getByRole('button', { name: 'cargo check --workspace', exact: true }).click();
    await tool.getByText('first line', { exact: true }).waitFor();
    assert.ok((await tool.locator('.acp-terminal-screen').boundingBox()).height < 80, 'short output fits its content');
    state.session.terminals.term.exit_status = { exitCode: 2 }; state.session.entries[1].content.status = 'failed'; state.session.revision++;
    await tool.getByLabel('退出码：2', { exact: true }).waitFor();
    assert.equal(await tool.getByLabel('执行中').count(), 0);
    assert.equal(await tool.locator('.acp-tool-card').evaluate(element => getComputedStyle(element).borderTopStyle), 'dashed');
    await tool.getByRole('button', { name: 'cargo check --workspace', exact: true }).click();
    await tool.getByLabel('退出码：2', { exact: true }).waitFor();
    assert.equal(await tool.locator('.xterm').count(), 0);
    if (process.env.ACP_SCREENSHOT_DIR) {
      await tool.getByRole('button', { name: 'cargo check --workspace', exact: true }).click();
      await tool.getByText('first line', { exact: true }).waitFor();
      await page.screenshot({ path: `${process.env.ACP_SCREENSHOT_DIR}/acp-terminal-tool.png` });
    }
  });
  await test('tool detail variants contain wide tables, fit diffs and suppress rejected output', async t => {
    const { page, state } = await fixture(t);
    const columns = Array.from({ length: 16 }, (_, index) => `Column ${index}`);
    const table = [columns.join(' | '), columns.map(() => '---').join(' | '), columns.map(() => '`long_unbroken_value`').join(' | ')].join('\n');
    const content = (id, kind, parts) => ({ id, kind: 'tool', content: { title: id, kind, status: 'completed', rawInput: { internal: 'must stay hidden for edits and images' }, content: parts } });
    state.session.entries = [
      content('Edit file', 'edit', [{ type: 'diff', path: '/repo/main.rs', oldText: 'old', newText: Array.from({ length: 35 }, (_, index) => `let value_${index} = ${index};`).join('\n') }]),
      content('Wide result', 'search', [{ type: 'content', content: { type: 'text', text: table } }]),
      content('Image result', 'read', [{ type: 'content', content: { type: 'image', mimeType: 'image/png', data: 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=' } }]),
    ];
    await newSession(page);
    await page.setViewportSize({ width: 750, height: 850 });
    const edit = page.getByRole('region', { name: 'Edit file', exact: true });
    await edit.getByRole('button', { name: 'Edit file', exact: true }).click();
    await edit.locator('.modified-in-monaco-diff-editor').waitFor();
    await page.waitForFunction(() => document.querySelector('.acp-diff')?.getBoundingClientRect().height > 320);
    assert.equal(await edit.getByText('Raw Input:', { exact: true }).count(), 0);
    const wide = page.getByRole('region', { name: 'Wide result', exact: true });
    await wide.getByRole('button', { name: 'Wide result', exact: true }).click();
    await wide.locator('table').waitFor();
    assert.equal(await wide.locator('table').evaluate(element => getComputedStyle(element).overflowX), 'auto');
    assert.equal(await page.locator('.acp-conversation').evaluate(element => element.scrollWidth <= element.clientWidth + 1), true);
    const image = page.getByRole('region', { name: 'Image result', exact: true });
    await image.getByRole('button', { name: 'Image result', exact: true }).click();
    await image.getByRole('img', { name: 'Agent image' }).waitFor();
    assert.equal(await image.getByText('Raw Input:', { exact: true }).count(), 0);
    state.session.entries[1].content.status = 'rejected'; state.session.revision++;
    await wide.locator('.acp-tool-output').waitFor({ state: 'detached' });
    assert.equal(await wide.getByLabel('失败或已取消').count(), 1);
  });
  const pasteImage = (input, { width = 4, height = 3, type = 'image/png', invalid = false } = {}) => input.evaluate(async (element, options) => {
    const canvas = document.createElement('canvas'); canvas.width = options.width; canvas.height = options.height;
    const context = canvas.getContext('2d'); context.fillStyle = '#f43'; context.fillRect(0, 0, canvas.width, canvas.height);
    const blob = options.invalid ? new Blob(['invalid'], { type: options.type }) : await new Promise(resolve => canvas.toBlob(resolve, options.type));
    const data = new DataTransfer(); data.items.add(new File([blob], 'image.png', { type: options.type }));
    element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  }, { width, height, type, invalid });
  await test('ACP image cut and paste preserve mixed content, native undo and the submitted payload on HTTP', async t => {
    const { page, state } = await fixture(t, false, { insecure: true });
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('before after <img src=x onerror=alert(1)>\n\nlast');
    await input.evaluate(element => {
      const range = document.createRange(); range.setStart(element.firstChild, 7); range.collapse(true);
      const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    });
    await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    await input.press('ControlOrMeta+a'); await input.press('ControlOrMeta+x');
    assert.equal(await input.textContent(), '');
    await input.press('ControlOrMeta+v');
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    assert.equal(await input.textContent(), 'before Image after <img src=x onerror=alert(1)>\n\nlast');
    assert.equal(await input.locator('img').count(), 0, 'clipboard text remains literal');
    await input.press('ControlOrMeta+z');
    assert.equal(await input.textContent(), '');
    await input.press('ControlOrMeta+Shift+z');
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    await input.press('Enter');
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    const content = interactions(state)[0].content;
    assert.deepEqual(content.map(block => block.type), ['text', 'image', 'text']);
    assert.equal(content[0].text, 'before ');
    assert.equal(content[2].text, ' after <img src=x onerror=alert(1)>\n\nlast');
    assert.ok(content[1].data.length > 0);
    assert.equal(await input.textContent(), '');
    await input.press('ControlOrMeta+v');
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    assert.equal(await input.textContent(), 'before Image after <img src=x onerror=alert(1)>\n\nlast', 'clipboard images outlive an accepted draft');
  });
  await test('copied ACP image selections survive source reload and paste into other editors with capability checks', async t => {
    const { page: source } = await fixture(t);
    const connected = source.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(source); await connected;
    const original = source.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await original.fill('Excluded text '); await pasteImage(original);
    await original.locator('.acp-image-mention[data-status="ready"]').waitFor();
    await original.evaluate(element => {
      const range = document.createRange(); range.selectNode(element.querySelector('[data-image-id]'));
      const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    });
    await original.press('ControlOrMeta+c');
    assert.equal(await original.textContent(), 'Excluded text Image ');
    await source.reload();
    const { page: unsupported } = await fixture(t, false, { imageSupport: false });
    await newSession(unsupported);
    const unsupportedInput = unsupported.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await unsupportedInput.fill('Keep this text'); await unsupportedInput.press('ControlOrMeta+v');
    await unsupported.getByRole('alert').getByText('This agent does not support images.', { exact: true }).waitFor();
    assert.equal(await unsupportedInput.textContent(), 'Keep this text');
    const { page, state } = await fixture(t);
    const targetConnected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await targetConnected;
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.focus(); await pasteImage(input, { width: 5 });
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    await input.press('ControlOrMeta+v'); await input.press('ControlOrMeta+v');
    assert.equal(await input.locator('.acp-image-mention[data-status="ready"]').count(), 3);
    await input.press('Enter');
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    const images = interactions(state)[0].content.filter(block => block.type === 'image');
    assert.equal(images.length, 3);
    assert.notEqual(images[0].data, images[1].data, 'pasted IDs cannot overwrite existing images');
    assert.equal(images[1].data, images[2].data);
    assert.equal(interactions(state)[0].content.some(block => block.text?.includes('Excluded')), false);
  });
  await test('invalid ACP clipboard payloads preserve the draft and external text stays literal', async t => {
    const { page } = await fixture(t);
    await newSession(page);
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('Keep draft');
    for (const payload of ['{broken', JSON.stringify({ version: 1, content: [{ type: 'image', mimeType: 'text/html', data: '<script>alert(1)</script>' }] })]) {
      await input.evaluate((element, payload) => {
        const data = new DataTransfer();
        data.setData('application/x-aow-acp-draft+json', payload); data.setData('text/plain', 'Image');
        element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
      }, payload);
      await page.getByRole('alert').getByText('Invalid ACP clipboard content. Copy the selection again.', { exact: true }).waitFor();
      assert.equal(await input.textContent(), 'Keep draft');
    }
    await input.press('ControlOrMeta+a');
    await input.evaluate(element => {
      const data = new DataTransfer();
      data.setData('text/html', '<img src=x onerror=alert(1)>'); data.setData('text/plain', '<b>external text</b>');
      element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
    });
    assert.equal(await input.textContent(), '<b>external text</b>');
    assert.equal(await input.locator('img, b').count(), 0);
  });
  await test('ACP image cut waits for pending conversion and retains the selection on failure', async t => {
    const { page } = await fixture(t);
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    await page.evaluate(() => {
      const decode = window.createImageBitmap.bind(window);
      const gate = new Promise(resolve => { window.finishImageDecode = resolve; });
      window.createImageBitmap = async (...args) => { await gate; return decode(...args); };
    });
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('Keep this '); await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="loading"]').waitFor();
    await input.press('ControlOrMeta+a'); await input.press('ControlOrMeta+x');
    await page.getByRole('alert').getByText('Wait for images to finish processing or remove failed images before copying or cutting.', { exact: true }).waitFor();
    assert.equal(await input.textContent(), 'Keep this Image ');
    await page.evaluate(() => window.finishImageDecode());
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    await input.press('ControlOrMeta+a'); await input.press('ControlOrMeta+x');
    assert.equal(await input.textContent(), '');
    await input.press('ControlOrMeta+v');
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    assert.equal(await input.textContent(), 'Keep this Image ');
  });
  await test('removed ACP image conversion cannot prevent clearing an accepted text draft', async t => {
    const { page, state } = await fixture(t);
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    await page.evaluate(() => {
      const decode = window.createImageBitmap.bind(window);
      const gate = new Promise(resolve => { window.finishImageDecode = resolve; });
      window.createImageBitmap = async (...args) => { await gate; return decode(...args); };
      const read = FileReader.prototype.readAsDataURL;
      window.imageConverted = new Promise(resolve => {
        FileReader.prototype.readAsDataURL = function (...args) {
          this.addEventListener('load', () => setTimeout(resolve, 0), { once: true });
          return read.apply(this, args);
        };
      });
    });
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.focus(); await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="loading"]').waitFor();
    await input.fill('Send only this text');
    let finish;
    t.after(() => finish?.());
    state.promptGate = new Promise(resolve => { finish = resolve; });
    const submitted = page.waitForRequest(request => new URL(request.url()).pathname.endsWith('/actions') && request.postDataJSON().action === 'prompt');
    await input.press('Enter'); await submitted;
    await page.evaluate(async () => { window.finishImageDecode(); await window.imageConverted; });
    finish();
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    assert.deepEqual(interactions(state)[0].content, [{ type: 'text', text: 'Send only this text' }]);
    assert.equal(await input.textContent(), '');
  });
  await test('pasted ACP images preserve caret order, native undo, preview, portal drafts and persisted display', async t => {
    const { page, state } = await fixture(t, false, { insecure: true });
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('before after');
    await input.evaluate(element => {
      const range = document.createRange(); range.setStart(element.firstChild, 7); range.collapse(true);
      const selection = window.getSelection(); selection.removeAllRanges(); selection.addRange(range);
    });
    await pasteImage(input, { width: 2000, height: 1000, type: 'image/jpeg' });
    const image = input.locator('.acp-image-mention[data-status="ready"]');
    await image.waitFor();
    assert.equal(await input.textContent(), 'before Image after');
    await input.press('ControlOrMeta+z');
    assert.equal(await input.locator('.acp-image-mention').count(), 0);
    await input.press('ControlOrMeta+Shift+z');
    await image.waitFor();
    await image.hover();
    const preview = page.getByRole('img', { name: 'Image preview', exact: true });
    await preview.waitFor();
    await page.waitForFunction(() => document.querySelector('.acp-image-preview img')?.naturalWidth > 0);
    assert.deepEqual(await preview.evaluate(image => [image.naturalWidth, image.naturalHeight]), [1568, 784]);
    assert.equal(await page.getByRole('tooltip').evaluate(element => { const rect = element.getBoundingClientRect(); return rect.top >= 0 && rect.bottom <= innerHeight && rect.left >= 0 && rect.right <= innerWidth; }), true);
    if (process.env.ACP_IMAGE_PREVIEW) await page.screenshot({ path: process.env.ACP_IMAGE_PREVIEW });
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await image.waitFor();
    assert.equal(await input.textContent(), 'before Image after');
    await page.getByRole('button', { name: '发送', exact: true }).click();
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    const content = interactions(state)[0].content;
    assert.deepEqual(content.map(block => block.type), ['text', 'image', 'text']);
    assert.equal(content[0].text, 'before ');
    assert.equal(content[2].text, ' after');
    assert.equal(content[1].mimeType, 'image/png');
    assert.ok(Buffer.from(content[1].data, 'base64').subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10])));
    assert.equal(content[1].uri, undefined);
    const displayed = page.locator('.acp-user-message img');
    await displayed.waitFor();
    const source = await displayed.getAttribute('src');
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await displayed.waitFor();
    assert.equal(await displayed.getAttribute('src'), source);
    assert.equal(interactions(state).length, 1);
  });
  await test('replayed ACP image mentions restore pasted images after session load without changing history', async t => {
    const { page, state } = await fixture(t);
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('Describe this image'); await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    await input.press('Enter');
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    const original = interactions(state)[0].content.find(block => block.type === 'image');
    const source = `data:${original.mimeType};base64,${original.data}`;
    const replayed = `Before **literal** <b>text</b>\n[@image]\n(${source})\nBetween\n![second](${source})\nAfter`;
    state.loadedEntries = [{ id: 'replayed-user', kind: 'user', content: { type: 'text', text: replayed } }];
    Object.assign(state.session, { entries: [], needs_load: true, status: 'disconnected' });
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await page.waitForFunction(() => {
      const images = [...document.querySelectorAll('.acp-user-message img')];
      return images.length === 2 && images.every(image => image.complete && image.naturalWidth === 4 && image.naturalHeight === 3);
    });
    assert.equal(state.loads, 1);
    const user = page.locator('.acp-user-message');
    assert.deepEqual(await user.locator('img').evaluateAll(images => images.map(image => image.src)), [source, source]);
    assert.equal(await user.textContent(), 'Before **literal** <b>text</b>\n\nBetween\n\nAfter');
    assert.equal(await user.locator('b, strong, a').count(), 0, 'surrounding user text stays literal');
    assert.equal(await page.getByRole('navigation', { name: '用户输入导航' }).getByRole('button').getAttribute('title'), 'Before **literal** <b>text</b>\n[图片]\nBetween\n[图片]\nAfter');
    assert.equal(state.session.entries[0].content.text, replayed, 'display recovery does not rewrite agent history');
    await page.reload();
    await page.getByRole('tab').filter({ hasText: 'Conversation' }).click();
    await user.locator('img').first().waitFor();
    assert.equal(await user.locator('img').count(), 2);
    assert.equal(interactions(state).length, 1, 'restoring images never resubmits the prompt');
  });
  await test('replayed ACP images work in history overviews and preserve malformed or unsupported data as text', async t => {
    const { page, state } = await fixture(t);
    const data = await page.evaluate(() => {
      const canvas = document.createElement('canvas'); canvas.width = 4; canvas.height = 3;
      return canvas.toDataURL('image/png').split(',')[1];
    });
    const invalid = ['[@image](data:image/png;base64,not*base64)', '[@image](data:image/png;base64,YmFk)', '[@image](data:image/svg+xml;base64,PHN2Zy8+)', '[@image](https://example.invalid/image.png)'].join('\n');
    const wrap = value => value.match(/.{1,32}/g).join('\r\n');
    const user = { id: 'u0', kind: 'user', content: { type: 'text', text: `[@image](data:image/png;base64,${wrap(data)})\n${invalid}` } };
    projectedHistory(state, 1);
    state.session.overview[0].user = [user];
    await newSession(page);
    await page.waitForFunction(() => {
      const images = [...document.querySelectorAll('.acp-user-message img')];
      return images.length === 1 && images[0].naturalWidth === 4;
    });
    assert.equal(await page.locator('.acp-user-message img').getAttribute('src'), `data:image/png;base64,${data}`);
    assert.equal(await page.locator('.acp-user-message').textContent(), `\n${invalid}`);
    assert.equal(state.processReads?.length || 0, 0, 'images render directly from the overview');
  });
  await test('replayed ACP images recover as chunks complete and damaged content is replaced', async t => {
    const { page, state } = await fixture(t);
    const sources = await page.evaluate(() => {
      const canvas = document.createElement('canvas'); canvas.width = 4; canvas.height = 3;
      return ['image/png', 'image/jpeg', 'image/webp'].map(type => canvas.toDataURL(type));
    });
    sources.push('data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7');
    const entry = { id: 'streamed-user', kind: 'user', content: { type: 'text', text: `[@image](${sources[0].slice(0, 60)}` } };
    state.session.entries = [entry];
    await newSession(page);
    const user = page.locator('.acp-user-message');
    await user.waitFor();
    assert.equal(await user.textContent(), entry.content.text);
    assert.equal(await user.locator('img').count(), 0, 'an incomplete wrapper stays text');
    for (const source of sources) {
      const damaged = '[@image](data:image/png;base64,YmFk)';
      entry.content.text = damaged; state.session.revision++;
      await user.getByText(damaged, { exact: true }).waitFor();
      assert.equal(await user.locator('img').count(), 0);
      entry.content.text = `[@image](${source})`; state.session.revision++;
      await page.waitForFunction(source => {
        const image = document.querySelector('.acp-user-message img');
        return image?.src === source && image.complete && image.naturalWidth > 0;
      }, source);
      assert.equal(await user.textContent(), '');
      assert.equal(await page.getByRole('navigation', { name: '用户输入导航' }).getByRole('button').getAttribute('title'), '[图片]');
    }
  });
  await test('image-only ACP prompts retain failed drafts and clear only accepted images', async t => {
    const { page, state } = await fixture(t);
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.focus(); await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    state.promptError = 'Image prompt rejected';
    await page.getByRole('button', { name: '发送', exact: true }).click();
    await page.getByRole('alert').getByText('Image prompt rejected', { exact: true }).waitFor();
    assert.equal(await input.locator('.acp-image-mention').count(), 1);
    state.promptError = '';
    await page.getByRole('button', { name: '发送', exact: true }).click();
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    assert.equal(await input.locator('.acp-image-mention').count(), 0);
    assert.equal(interactions(state).at(-1).content[0].type, 'image');
    await input.focus(); await pasteImage(input, { invalid: true });
    await page.getByRole('alert').getByText('Failed to decode image.', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '发送', exact: true }).isDisabled(), true);
    await input.getByRole('button', { name: 'Image', exact: true }).focus();
    await page.keyboard.press('Backspace');
    assert.equal(await input.locator('.acp-image-mention').count(), 0);
    await input.fill('Still editable');
    assert.equal(await page.getByRole('button', { name: '发送', exact: true }).isEnabled(), true);
  });
  await test('pending ACP image conversion survives moving the view and late acceptance preserves newer image drafts', async t => {
    const { page, state } = await fixture(t);
    const connected = page.waitForResponse(response => response.url().endsWith('/connection'));
    await newSession(page); await connected;
    await page.evaluate(() => {
      const decode = window.createImageBitmap.bind(window);
      const gate = new Promise(resolve => { window.finishImageDecode = resolve; });
      window.createImageBitmap = async (...args) => { await gate; return decode(...args); };
    });
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('First '); await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="loading"]').waitFor();
    assert.equal(await page.getByRole('button', { name: '发送', exact: true }).isDisabled(), true);
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await input.locator('.acp-image-mention[data-status="loading"]').waitFor();
    await page.evaluate(() => window.finishImageDecode());
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    let finish;
    t.after(() => finish?.());
    state.promptGate = new Promise(resolve => { finish = resolve; });
    const submitted = page.waitForRequest(request => new URL(request.url()).pathname.endsWith('/actions') && request.postDataJSON().action === 'prompt');
    await page.getByRole('button', { name: '发送', exact: true }).click(); await submitted;
    await input.fill('New draft '); await pasteImage(input);
    await input.locator('.acp-image-mention[data-status="ready"]').waitFor();
    finish();
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    assert.equal(await input.textContent(), 'New draft Image ');
    assert.equal(await input.locator('.acp-image-mention[data-status="ready"]').count(), 1);
  });
  await test('agents without image capability reject image paste and preserve plain text and IME input', async t => {
    const { page, state } = await fixture(t, false, { imageSupport: false });
    await newSession(page);
    const input = page.getByRole('textbox', { name: 'ACP 消息', exact: true });
    await input.fill('Keep this'); await pasteImage(input);
    await page.getByRole('alert').getByText('This agent does not support images.', { exact: true }).waitFor();
    assert.equal(await input.innerText(), 'Keep this');
    await input.fill('first'); await input.press('Shift+Enter'); await input.pressSequentially('second');
    await input.evaluate(element => element.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', isComposing: true, bubbles: true })));
    assert.equal(interactions(state).length, 0);
    await input.press('Enter');
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    assert.deepEqual(interactions(state)[0].content, [{ type: 'text', text: 'first\nsecond' }]);
  });
  await test('slash commands use advertised suggestions and do not submit until explicitly sent', async t => {
    const { page, state } = await fixture(t);
    state.session.commands = [{ name: 'review', description: 'Review the changes' }, { name: 'help', description: 'Show help' }];
    await newSession(page);
    const input = page.getByLabel('ACP 消息', { exact: true });
    await input.fill('/rev');
    await page.getByRole('option', { name: 'review Review the changes', exact: true }).waitFor();
    await input.press('Enter');
    assert.equal(await input.innerText(), '/review ');
    assert.equal(interactions(state).length, 0);
    await input.press('Enter');
    await page.getByText('Hello from ACP', { exact: true }).waitFor();
    assert.equal(interactions(state)[0].content[0].text, '/review ');
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
    assert.equal(await input.innerText(), 'Keep this unsent draft');
    assert.equal(state.sessions.length, 1);
    assert.deepEqual(interactions(state), []);
    // Authentication completed elsewhere on the shared connection also removes the callout.
    Object.assign(state.session, { auth_required: true, error: '请重新登录', revision: state.session.revision + 1 });
    await content.getByRole('region', { name: 'Agent 登录', exact: true }).waitFor();
    Object.assign(state.session, { auth_required: false, error: null, revision: state.session.revision + 1 });
    await input.waitFor({ state: 'visible' });
    assert.equal(await input.innerText(), 'Keep this unsent draft');
    assert.deepEqual(interactions(state), []);
  });
  await test('history refresh and reload use only local records even when the agent is offline', async t => {
    const { page, state } = await fixture(t);
    assert.equal(state.connections, 0); assert.deepEqual(state.listCalls, []);
    await newSession(page);
    await page.getByLabel('ACP 消息', { exact: true }).fill('Offline draft');
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
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 0); assert.deepEqual(interactions(state), []);
    await dialog.getByLabel('导入 Test Agent', { exact: true }).uncheck();
    assert.equal(await dialog.getByRole('button', { name: '导入 0 个会话', exact: true }).isDisabled(), true);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(state.imports.length, 0);
    await importHistory(page);
    assert.equal(state.sessions.length, 2); assert.equal(state.loads, 0); assert.deepEqual(interactions(state), []);
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
    const resuming = page.waitForRequest(request => new URL(request.url()).pathname.endsWith('/actions') && request.postDataJSON().action === 'resume');
    await content.getByRole('button', { name: '登录 Agent', exact: true }).click();
    await resuming;
    await page.getByRole('button', { name: '移动会话视图', exact: true }).click();
    await content.getByRole('status').getByText(/正在恢复会话…/).waitFor();
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
    await content.getByRole('button', { name: '重试连接', exact: true }).click();
    await content.getByText('Imported conversation content', { exact: true }).waitFor();
    assert.equal(state.sessions.length, 1); assert.equal(state.loads, 1);
  });
  await test('cancelled recovery keeps the old conclusion, shows cleanup, and can retry', async t => {
    const { page, state } = await fixture(t, false, { recoveryLifecycle: true });
    await importHistory(page);
    const saved = state.sessions[0];
    saved.entries = [
      { id: 'old-user', kind: 'user', content: { type: 'text', text: 'Earlier question' } },
      { id: 'old-answer', kind: 'assistant', message_id: 'answer', content: { type: 'text', text: 'Earlier completed answer' } },
    ];
    state.resumeGate = new Promise(resolve => { state.finishResume = resolve; });
    t.after(() => state.finishResume());
    await page.getByRole('button', { name: '打开 ACP 会话 Earlier conversation', exact: true }).click();
    const content = page.getByRole('region', { name: 'ACP 会话内容', exact: true });
    await content.getByRole('button', { name: '取消恢复', exact: true }).click();
    await content.getByRole('status').getByText(/正在等待上次恢复结束并清理/).waitFor();
    await content.getByText('Earlier completed answer', { exact: true }).waitFor();
    assert.equal(state.actions.filter(action => action.action === 'resume').length, 1);
    state.finishResume();
    await content.getByRole('alert').getByText('会话恢复失败：Recovery cancelled', { exact: true }).waitFor();
    assert.equal(await content.getByText('本轮执行失败，未生成最终结论。', { exact: true }).count(), 0);
    assert.equal(state.sessions.length, 1);
    state.cancelledRecovery = false; state.resumeGate = undefined;
    await content.getByRole('button', { name: '重试连接', exact: true }).click();
    await content.getByText('Imported conversation content', { exact: true }).waitFor();
    assert.equal(await content.getByText('会话恢复失败：Recovery cancelled', { exact: true }).count(), 0);
    assert.deepEqual(state.actions.map(action => action.action), ['resume', 'cancel', 'resume']);
  });
  await test('bounded logs retain response names without the original request line', async t => {
    const { page } = await fixture(t);
    const result = await page.evaluate(async () => {
      const { logMessages } = await import('/src/features/acp/acp_tools/messages.ts');
      return logMessages([{ timestamp: 'now', direction: 'incoming', connection_id: 'connection', request_method: 'session/load', message: JSON.stringify({ id: 'old-request', result: {} }) }])[0];
    });
    assert.equal(result.name, 'session/load');
    assert.equal(result.type, 'Response');
    assert.equal(result.failed, false);
  });
  await test('an unavailable agent does not prevent importing another agent', async t => {
    const { page, state } = await fixture(t, false, { agents: ['test', 'offline'].map(id => ({ id, name: id === 'test' ? 'Test Agent' : 'Offline Agent', supported: true, installed: true })) });
    await page.route('**/api/acp/connections', async route => {
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
    await page.route('**/api/acp/connections', async route => {
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
