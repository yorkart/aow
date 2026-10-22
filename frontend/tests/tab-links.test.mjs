import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';
import { fillInboxEditor } from './fixtures/inbox-editor.mjs';
import { acpControlFixture } from './fixtures/acp-control.mjs';

const screenshotDir = process.env.AOW_TEST_SCREENSHOT_DIR || '/tmp';
await mkdir(screenshotDir, { recursive: true });
const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error',
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
const worktrees = ['/workspace/main', '/workspace/linked'].map((path, i) => ({
  id: `wt-${i}`, project_id: 'project', path, branch: i ? 'dev' : 'main', head: 'abc',
  is_main: !i, detached: false, locked: false, prunable: false, color: 'default',
}));
const project = { id: 'project', name: 'Link Fixture', registered_path: worktrees[0].path,
  common_git_dir: '/workspace/.git', notes_path: '/notes', worktrees };
function terminal(id, root, name) {
  return { id, name, name_is_custom: true, workspace_root: root, revision: 1,
    layout: { type: 'pane', pane_id: `${id}-pane` },
    panes: [{ id: `${id}-pane`, name, name_is_custom: true, cwd: root, kind: 'terminal',
      shell: '/bin/bash', status: 'running', rows: 24, cols: 80 }] };
}
function acpSession(id = 'acp-local', cwd = '/workspace/linked') {
  return { thread_id: id, session_id: `remote-${id}`, agent_id: 'test', cwd, title: `Saved ACP ${id}`, status: 'disconnected', revision: 1, updated_at: '',
    entries: [{ id: 'user', kind: 'user', content: { type: 'text', text: 'Read the workspace' } },
      { id: 'reply', kind: 'assistant', content: { type: 'text', text: `Retained reply for ${id}` } }],
    permissions: [], modes: {}, config_options: [], commands: [], usage: null };
}
async function assertInboxAutosave(scope) {
  assert.equal(await scope.getByRole('button', { name: /^(提交|保存|取消)$/ }).count(), 0);
}

let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  async function fixture(t, { mobile = false, login = false, floating = false, extraFloatingTabs = [], global = false, missing = false, removedWorkspace = false, temporary = false, entry = 'target', editable = false, tabUrl, fileContents = {}, sessionMarkdown = 'Restored conclusion', sessionAgent = false, pinned = [], reviewTargets = [], preserveFloating = false, acp = false, acpHistory = [], acpSavedTabs = [], restoreAcpSessions = false, terminalUnavailable = false } = {}) {
    const context = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1440, height: 900 } });
    await context.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } });
    const root = temporary ? '/private/tmp/aow-inbox-link' : global ? '/global' : worktrees[1].path;
    const target = terminal('target', root, 'Target Tab');
    const other = terminal('other', root, 'Other Tab');
    const tabs = [terminal('main-tab', worktrees[0].path, 'Main Tab'), other, target];
    const projects = removedWorkspace ? [] : [project];
    if (global) projects.push({ ...project, id: '__aow_floating', name: '浮动工作区', builtin: true,
      registered_path: root, worktrees: [{ ...worktrees[0], id: 'global-main', project_id: '__aow_floating', path: root }] });
    const state = { authenticated: !login, targetReads: 0, mutations: [], errors: [], reads: [], inbox: [], acpSessions: structuredClone(acpHistory), acpListError: '', acpConnections: 0, acpWrites: [], terminalUnavailable };
    const session = { id: 'codex:session-one', session_id: 'session-one', agent: 'codex', title: 'Linked Conversation', cwd: '/actual-session', created_at: '', updated_at: '' };
    const task = { id: 'task-one', name: 'Linked Automation', project_id: 'project', project_name: project.name, revision: 1, agent: 'codex', workspace_mode: 'existing', workspace_path: root, prompt: 'Task prompt', cron: '0 9 * * *', interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: false, base_branch: '', cleanup_worktree: false, is_running: false, scheduler_error: null, last_run: null, next_run_at: null };
    const run = { id: 'run-old', task_id: task.id, task_name: task.name, agent: 'codex', source: 'manual', status: 'completed', started_at: '2026-09-19T00:00:00Z', finished_at: '2026-09-19T00:01:00Z', workspace_path: root, branch: 'dev', session_id: null, duration_ms: 60000, exit_code: 0 };
    const pr = { number: 42, title: 'Linked PR', source_branch: 'dev', target_branch: 'main', status: 'open', draft: false, created_at: '', updated_at: '', url: null, description: 'PR description', files: [], checks: [], reviewers: [], threads: [], unresolved_threads: [], changes_count: 0, commits_count: 1 };
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); if (!editable) assert.deepEqual(state.mutations, []); });
    await installLiveEvents(context);
    const controls = acpControlFixture();
    if (acp) await controls.install(context);
    await context.addInitScript(({ root, floating, extraFloatingTabs, preserveFloating, acp, acpSavedTabs, restoredSessions }) => {
      window.sendTaskStop = data => window.emitLiveEvent('task-stopped', data);
      window.taskStopReady = () => window.liveEventSockets.some(socket => socket.readyState === 1);
      if (!acp || !localStorage.getItem('aow-active')) localStorage.setItem('aow-active', '/workspace/main');
      if (!acp || !localStorage.getItem(`aow-workspace-tabs:${root}`)) localStorage.setItem(`aow-workspace-tabs:${root}`, JSON.stringify({ active: 'terminal:other' }));
      if (acpSavedTabs.length && !localStorage.getItem(`aow-acp-tabs:${root}`)) localStorage.setItem(`aow-acp-tabs:${root}`, JSON.stringify(acpSavedTabs));
      for (const workspace of new Set(restoredSessions.map(session => session.cwd))) {
        if (localStorage.getItem(`aow-acp-tabs:${workspace}`)) continue;
        const tabs = restoredSessions.filter(session => session.cwd === workspace).map(session => ({
          id: session.thread_id, thread_id: session.thread_id, agentId: session.agent_id, title: session.title,
        }));
        localStorage.setItem(`aow-acp-tabs:${workspace}`, JSON.stringify(tabs));
        localStorage.setItem(`aow-workspace-tabs:${workspace}`, JSON.stringify({ active: `acp:${tabs[0].id}` }));
      }
      sessionStorage.setItem(`aow.mobile.terminal.${root}`, 'other:other-pane');
      if (floating && (!preserveFloating || !localStorage.getItem('aow-floating-tabs-v1'))) {
        localStorage.setItem('aow-floating-tabs-v1', JSON.stringify([{ workspace: root,
          id: 'terminal:target', targetId: 'target', kind: 'terminal', label: 'Target Tab' }, ...extraFloatingTabs]));
        localStorage.setItem('aow-floating-open', 'false');
      }
    }, { root, floating, extraFloatingTabs, preserveFloating, acp, acpSavedTabs, restoredSessions: restoreAcpSessions ? acpHistory : [] });
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      let data;
      if (request.method() === 'GET') state.reads.push(url.pathname + url.search);
      if (url.pathname.startsWith('/api/acp/') && request.method() !== 'GET') state.acpWrites.push(request.method() + ' ' + url.pathname);
      if (request.method() !== 'GET' && /\/api\/(fs|aow\/automations)\//.test(url.pathname)) state.mutations.push(request.method() + url.pathname);
      if (url.pathname === '/api/auth/status') data = { configured: true, authenticated: state.authenticated };
      else if (url.pathname === '/api/auth/login') { state.authenticated = true; data = { configured: true, authenticated: true }; }
      else if (!state.authenticated) { await route.fulfill({ status: 401, json: { message: 'Login required' } }); return; }
      else if (acp && url.pathname === '/api/acp/agents') data = [{ id: 'test', name: 'Test Agent', supported: true, configured: true, installed: true }];
      else if (acp && url.pathname === '/api/acp/settings') data = { content: '{}', revision: '1', path: '/config/acp/settings.json' };
      else if (acp && url.pathname === '/api/acp/sessions' && request.method() === 'GET') {
        if (state.acpListError) { await route.fulfill({ status: 503, json: { message: state.acpListError } }); return; }
        data = state.acpSessions.filter(session => session.cwd === url.searchParams.get('cwd'));
      }
      else if (acp && url.pathname === '/api/acp/connections') {
        state.acpConnections++; data = { id: 'acp-connection', agent_id: 'test', cwd: root, auth_methods: [], capabilities: {} };
      }
      else if (acp && url.pathname === '/api/acp/sessions' && request.method() === 'POST') {
        assert.deepEqual(request.postDataJSON(), { agent_id: 'test', cwd: root });
        data = { thread_id: 'acp-local', session_id: null, agent_id: 'test', cwd: root, title: 'ACP Conversation', status: 'disconnected', revision: 1, updated_at: '', entries: [], permissions: [], modes: {}, config_options: [], commands: [], usage: null };
        state.acpSessions.push(data);
      }
      else if (acp && /^\/api\/acp\/sessions\/[^/]+\/actions$/.test(url.pathname)) {
        const id = decodeURIComponent(url.pathname.split('/').at(-2));
        assert.equal(controls.authorized(id, request), true);
        assert.equal(request.postDataJSON().action, 'resume');
        data = state.acpSessions.find(session => session.thread_id === id);
        if (!data.session_id) { state.acpConnections++; data.session_id = 'acp-remote'; }
        data.status = 'idle'; data.revision++;
      }
      else if (acp && /^\/api\/acp\/sessions\/[^/]+\/connection$/.test(url.pathname)) data = null;
      else if (acp && /^\/api\/acp\/sessions\/[^/]+\/metadata$/.test(url.pathname)) {
        data = state.acpSessions.find(session => session.thread_id === decodeURIComponent(url.pathname.split('/').at(-2)));
        if (!data) { await route.fulfill({ status: 404, json: { message: '该 ACP 会话已不存在。' } }); return; }
      }
      else if (acp && /^\/api\/acp\/sessions\/[^/]+$/.test(url.pathname)) {
        data = state.acpSessions.find(session => session.thread_id === decodeURIComponent(url.pathname.split('/').at(-1)));
        if (!data) { await route.fulfill({ status: 404, json: { message: 'ACP session not found' } }); return; }
        if (request.method() === 'DELETE') {
          if (state.acpDeleteError) { await route.fulfill({ status: 503, json: { message: state.acpDeleteError } }); return; }
          state.acpSessions = state.acpSessions.filter(session => session !== data);
          data = {};
        }
      }
      else if (acp && url.pathname === '/api/acp/connection-status') data = null;
      else if (acp && url.pathname === '/api/acp/connections/acp-connection/requests') data = [];
      else if (url.pathname === '/api/inbox') data = { revision: state.inbox.length, items: state.inbox, labels: [], comment_counts: Object.fromEntries(state.inbox.map(item => [item.id, 0])), executions: temporary ? [{ id: 'inbox-run', workspace_mode: 'temporary', tab_id: 'target', cwd: root }] : [] };
      else if (url.pathname === '/api/inbox/items' && request.method() === 'POST') {
        const input = request.postDataJSON();
        data = { id: `inbox-${state.inbox.length}`, markdown: input.markdown, project_id: null, label_ids: [], revision: 1, created_at: '', updated_at: '' };
        state.inbox.push(data);
      }
      else if (url.pathname === '/api/aow/projects') data = projects;
      else if (url.pathname === '/api/aow/agents') data = sessionAgent ? [{ id: 'codex', display_name: 'Codex', available: true, args: [], env: {} }] : [];
      else if (url.pathname === '/api/aow/settings') data = { notes_base: '/notes', execution_path: ['/usr/bin'] };
      else if (url.pathname.includes('/pinned-')) data = { paths: url.pathname.endsWith('/pinned-worktrees') ? pinned : [], revision: 0 };
      else if (url.pathname === '/api/terminals/task-stops') { await route.fulfill({ contentType: 'text/event-stream', body: ': ready\n\n' }); return; }
      else if (url.pathname === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
      else if (/^\/api\/terminals\/[^/]+$/.test(url.pathname) && request.method() === 'GET') {
        const id = decodeURIComponent(url.pathname.split('/').at(-1));
        if (id === 'target') state.targetReads++;
        data = tabs.find(tab => tab.id === id);
        if (!data || missing) { await route.fulfill({ status: 404, json: { message: 'Tab not found' } }); return; }
      }
      else if (url.pathname === '/api/terminals' && request.method() === 'GET') {
        if (state.terminalUnavailable) { await route.fulfill({ status: 503, json: { message: 'Terminal inventory unavailable' } }); return; }
        data = tabs.filter(tab => !url.searchParams.has('workspace_root') || tab.workspace_root === url.searchParams.get('workspace_root'));
      }
      else if (editable && url.pathname === '/api/terminals' && request.method() === 'POST') {
        data = terminal('created', request.postDataJSON().workspace_root, 'Created Tab');
        tabs.push(data); state.mutations.push('create');
      }
      else if (editable && /^\/api\/terminals\/[^/]+$/.test(url.pathname) && request.method() === 'DELETE') {
        const index = tabs.findIndex(tab => tab.id === url.pathname.split('/').at(-1));
        assert.ok(index >= 0); tabs.splice(index, 1); state.mutations.push('close'); data = { ok: true };
      }
      else if (url.pathname.startsWith('/api/terminals') && request.method() !== 'GET') state.mutations.push(request.method() + url.pathname);
      else if (url.pathname.startsWith('/api/fs/tree') || url.pathname === '/api/fs/home') {
        const path = url.pathname === '/api/fs/home' ? '/home/fixture' : decodeURIComponent(url.pathname.slice('/api/fs/tree'.length));
        data = { path, entries: [{ name: 'read me #中文.md', path: path.replace(/\/$/, '') + '/read me #中文.md', kind: 'file', size: 12, readonly: false }] };
      }
      else if (url.pathname.startsWith('/api/fs/text')) {
        const path = decodeURIComponent(url.pathname.slice('/api/fs/text'.length));
        data = { path, content: fileContents[path] ?? '# Linked document\n\nRestored from URL.', version: 'v1', language: 'markdown', mime: 'text/markdown', size: 38 };
      }
      else if (url.pathname === '/api/git/diff' || url.pathname === '/api/git/commit/diff') data = { repository: url.searchParams.get('repo'), path: url.searchParams.get('path'), staged: url.searchParams.get('staged') === 'true', patch: '@@ -1 +1 @@\n-before\n+after\n', original: 'before\n', modified: 'after\n', truncated: false };
      else if (url.pathname === '/api/review-targets') data = reviewTargets;
      else if (url.pathname === '/api/my-pull-requests') data = { repository: root, current_branch: 'dev', current_user: { id: 'u', username: 'me' }, pull_requests: [{ ...pr, provider: url.searchParams.get('provider') || 'github', remote: url.searchParams.get('remote') || 'origin' }] };
      else if (url.pathname === '/api/my-pull-requests/42') {
        if (url.searchParams.get('provider') === 'missing') { await route.fulfill({ status: 503, json: { message: 'Provider 未配置或已停用。' } }); return; }
        data = { ...pr, provider: url.searchParams.get('provider') || 'github', remote: url.searchParams.get('remote') || 'origin' };
      }
      else if (url.pathname === '/api/aow/agent-sessions') data = url.searchParams.get('agent') === 'codex' ? [session] : [];
      else if (url.pathname.endsWith('/session-one/snapshot')) data = { ...session, status: 'completed', turns: [{ id: 'turn', status: 'completed', user: { text: 'Input', timestamp: null }, final: { text: sessionMarkdown, timestamp: null } }], truncated: false, captured_at: '' };
      else if (url.pathname === '/api/aow/automations/status') data = { platform: 'systemd', ready: true, message: '', timezone: 'Asia/Shanghai' };
      else if (url.pathname === '/api/aow/automations') data = [task];
      else if (url.pathname === '/api/aow/automations/task-one') data = task;
      else if (url.pathname === '/api/aow/automations/task-one/runs') data = []; // Target run is beyond the first page.
      else if (url.pathname === '/api/aow/automations/task-one/runs/run-old') data = run;
      else if (url.pathname === '/api/git/ignored') data = { repository: root, ignored: [] };
      else if (url.pathname === '/api/git/status') data = { repository: root, branch: 'dev', files: [] };
      else if (url.pathname === '/api/git/repositories') data = [];
      else if (url.pathname === '/api/git/log') data = { commits: [], repository: root, upstream: null };
      await route.fulfill(data === undefined ? { status: 404, json: { message: `No fixture: ${url.pathname}` } } : { json: data });
    });
    await context.routeWebSocket('**/api/terminals/**', socket => {
      socket.onMessage(message => {
        if (typeof message !== 'string' || JSON.parse(message).type !== 'claim') return;
        socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
        socket.send(JSON.stringify({ type: 'stream', epoch: 'fixture', offset: 0, reset: true, replay_bytes: 0,
          restore_cols: 80, restore_rows: 24, restore: '\x1b[2J\x1b[HExisting terminal\r\n$ ' }));
      });
    });
    const page = await context.newPage();
    await page.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } });
    page.setDefaultTimeout(15000);
    page.on('pageerror', error => state.errors.push(`${page.url()}\n${error.stack ?? error.message}`));
    const destination = new URL(tabUrl ?? `/aow/${entry ? `tabs/${entry}` : ''}`, base);
    destination.searchParams.set('ui', mobile ? 'mobile' : 'desktop');
    await page.goto(destination.href);
    return { page, state, root };
  }

  await test('ACP sessions publish their persisted identity and restore without creating a duplicate', async t => {
    const { page, state } = await fixture(t, { acp: true });
    await page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab.active').filter({ hasText: 'Target Tab' }).waitFor();
    await page.getByRole('button', { name: '隐藏右侧栏', exact: true }).click();
    await page.getByRole('button', { name: '新建窗体', exact: true }).click();
    await page.getByRole('button', { name: '新建 ACP 会话：Test Agent', exact: true }).click();
    const active = page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab.active').filter({ hasText: 'ACP Conversation' });
    await active.waitFor();
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local' && !url.searchParams.has('workspace'));
    assert.equal(await page.locator('.project-aow-new-menu').count(), 0);
    await page.getByRole('button', { name: '显示右侧栏', exact: true }).click();
    assert.equal(await page.getByRole('button', { name: '新建 ACP 会话', exact: true }).count(), 0);
    const link = page.url();
    await page.screenshot({ path: join(screenshotDir, 'aow-acp-panel-theme.png') });
    await page.reload();
    await active.waitFor();
    await page.getByLabel('ACP 消息', { exact: true }).waitFor();
    await page.getByRole('button', { name: 'ACP 面板', exact: true }).click();
    await page.getByRole('button', { name: '打开 ACP 会话 ACP Conversation', exact: true }).click();
    assert.equal(await page.getByRole('tab', { name: /^ACP Conversation/ }).count(), 1);
    await page.getByRole('button', { name: '关闭 ACP Conversation', exact: true }).click();
    assert.equal(state.acpConnections, 1);
    assert.equal(state.acpWrites.filter(path => path.endsWith('/sessions')).length, 1);
    await page.goto(link);
    await active.waitFor();
    assert.equal(await page.getByRole('tab', { name: /^ACP Conversation/ }).count(), 1);
    assert.equal(state.acpSessions.length, 1);
    assert.deepEqual(state.mutations, []);
  });

  await test('floating workspace creates ACP sessions in the global workspace with its sidebar closed', async t => {
    const { page, state } = await fixture(t, { acp: true, global: true, floating: true, preserveFloating: true });
    const floating = page.locator('[data-floating-workspace]');
    await floating.getByRole('tab', { name: /^Target Tab/ }).waitFor();
    const sidebarToggle = floating.getByRole('button', { name: '全局项目面板', exact: true });
    if (await sidebarToggle.getAttribute('aria-pressed') === 'true') await sidebarToggle.click();
    await floating.getByRole('button', { name: '浮动工作区新建', exact: true }).click();
    await floating.getByRole('button', { name: '新建 ACP 会话：Test Agent', exact: true }).click();
    await floating.getByLabel('ACP 消息', { exact: true }).waitFor();
    await floating.getByRole('tab', { name: /^ACP Conversation/ }).waitFor();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local');
    assert.equal(await sidebarToggle.getAttribute('aria-pressed'), 'false');
    assert.equal(await floating.locator('.project-aow-new-menu').count(), 0);
    assert.equal(state.acpSessions[0].cwd, '/global');
    assert.equal(state.acpSessions.length, 1);
    await page.reload();
    await floating.getByLabel('ACP 消息', { exact: true }).waitFor();
    assert.equal(state.acpWrites.filter(path => path.endsWith('/sessions')).length, 1);
  });

  await test('ACP links open saved sessions in a fresh browser after login on desktop and mobile', async t => {
    for (const mobile of [false, true]) {
      const { page, state } = await fixture(t, { acp: true, mobile, login: true, acpHistory: [acpSession()], tabUrl: '/aow/tabs/acp/acp-local' });
      await page.getByLabel('账号').fill('admin');
      await page.getByLabel('密码', { exact: true }).fill('test-password');
      await page.getByRole('button', { name: '登录', exact: true }).click();
      await page.getByText('Retained reply for acp-local', { exact: true }).waitFor();
      await page.getByLabel('ACP 消息', { exact: true }).waitFor();
      await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local' && !url.searchParams.has('workspace'));
      await page.screenshot({ path: join(screenshotDir, `aow-acp-link-${mobile ? 'mobile' : 'desktop'}.png`) });
      await page.reload();
      await page.getByText('Retained reply for acp-local', { exact: true }).waitFor();
      assert.deepEqual(state.acpWrites, ['POST /api/acp/sessions/acp-local/actions'], 'the owner restores once without creating or prompting');
      assert.equal(state.acpConnections, 0);
      assert.deepEqual(state.errors, []);
    }
  });

  await test('desktop ACP project inventory opens a foreign worktree in its owning workspace', async t => {
    const main = { ...acpSession('main', '/workspace/main'), source: 'cli', created_at: '2026-01-01' };
    const linked = { ...acpSession('linked'), source: 'user', created_at: '2026-01-02' };
    const { page, state } = await fixture(t, { acp: true, acpHistory: [main, linked] });
    await page.getByRole('button', { name: 'ACP 面板', exact: true }).click();
    const list = page.locator('.project-aow-surface:not([hidden])').getByRole('region', { name: 'ACP 会话列表', exact: true });
    await list.getByRole('button', { name: '打开 ACP 会话 Saved ACP main', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/main');
    await page.getByText('Retained reply for main', { exact: true }).waitFor();
    assert.equal(await list.locator('.instance-worktree-heading[aria-current="true"]').first().getAttribute('title'), '/workspace/main\nmain');
    assert.equal(state.acpSessions.length, 2);
    assert.deepEqual(state.acpWrites, ['POST /api/acp/sessions/main/actions']);
    assert.deepEqual(state.errors, []);
  });

  async function refreshOnActivation(page, event) {
    await Promise.all([
      page.waitForResponse(response => new URL(response.url()).pathname === '/api/aow/projects'),
      page.evaluate(event => {
        if (event === 'focus') window.dispatchEvent(new Event('focus'));
        else {
          Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
          document.dispatchEvent(new Event('visibilitychange'));
          Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
          document.dispatchEvent(new Event('visibilitychange'));
        }
      }, event),
    ]);
    // Allow the refreshed inventory's React effects to apply before asserting.
    await page.waitForTimeout(250);
  }

  for (const terminalUnavailable of [false, true]) {
    await test(`ACP worktree selection survives focus and visibility refresh with terminal inventory ${terminalUnavailable ? 'unavailable' : 'ready'}`, async t => {
      const main = { ...acpSession('main', '/workspace/main'), status: 'idle' };
      const linked = { ...acpSession('linked'), status: 'idle' };
      const { page, state } = await fixture(t, { acp: true, acpHistory: [main, linked], restoreAcpSessions: true,
        terminalUnavailable, tabUrl: '/aow/tabs/acp/main' });
      const active = page.locator('.project-aow-surface:not([hidden])');
      await active.getByText('Retained reply for main', { exact: true }).waitFor();
      if (terminalUnavailable) await active.locator('.project-aow-inline-error').filter({ hasText: 'Terminal inventory unavailable' }).waitFor();
      else await active.getByRole('tab', { name: /^Main Tab/ }).waitFor();
      await page.locator('.project-aow-worktrees button[title="/workspace/linked"]').click();
      await active.getByText('Retained reply for linked', { exact: true }).waitFor();
      await page.waitForURL(url => url.pathname === '/aow/tabs/acp/linked');
      await active.getByLabel('ACP 消息', { exact: true }).fill('Keep this linked draft');
      for (const event of ['focus', 'visibilitychange']) {
        await refreshOnActivation(page, event);
        assert.equal(await page.locator('.project-aow-worktrees button.active').getAttribute('title'), linked.cwd);
        assert.equal(new URL(page.url()).pathname, '/aow/tabs/acp/linked');
        assert.equal(await active.getByLabel('ACP 消息', { exact: true }).innerText(), 'Keep this linked draft');
        assert.equal(await active.getByRole('tab', { selected: true }).innerText(), linked.title);
      }
      if (terminalUnavailable) {
        state.terminalUnavailable = false;
        await refreshOnActivation(page, 'visibilitychange');
        await active.getByRole('tab', { name: /^Other Tab/ }).waitFor();
        assert.equal(new URL(page.url()).pathname, '/aow/tabs/acp/linked');
        assert.equal(await active.getByLabel('ACP 消息', { exact: true }).innerText(), 'Keep this linked draft');
      }
      assert.deepEqual(await page.locator('.project-aow-worktrees > button').evaluateAll(buttons => buttons.map(button => button.title)), worktrees.map(worktree => worktree.path));
      assert.deepEqual(state.acpWrites, []);
    });
  }

  await test('fresh ACP links and same-worktree session switching do not wait for terminal inventory', async t => {
    const one = { ...acpSession('one'), status: 'idle' };
    const two = { ...acpSession('two'), status: 'idle' };
    const { page, state } = await fixture(t, { acp: true, acpHistory: [one, two], terminalUnavailable: true,
      tabUrl: '/aow/tabs/acp/one' });
    const active = page.locator('.project-aow-surface:not([hidden])');
    await active.getByText('Retained reply for one', { exact: true }).waitFor();
    await active.getByRole('button', { name: '打开 ACP 会话 Saved ACP two', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/two');
    await active.getByText('Retained reply for two', { exact: true }).waitFor();
    await active.getByRole('tab', { name: /^Saved ACP one/ }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/one');
    await refreshOnActivation(page, 'focus');
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/acp/one');
    assert.deepEqual(state.acpWrites, []);
  });

  for (const selection of ['worktree', 'tab']) {
    await test(`explicit ${selection} selection cancels a terminal link waiting for its first inventory`, async t => {
      const main = { ...acpSession('main', '/workspace/main'), status: 'idle' };
      const second = { ...acpSession('second', '/workspace/main'), status: 'idle' };
      const linked = { ...acpSession('linked'), status: 'idle' };
      const { page, state } = await fixture(t, { acp: true, acpHistory: [main, second, linked], restoreAcpSessions: true,
        terminalUnavailable: true, pinned: ['/workspace/linked'], tabUrl: '/aow/tabs/main-tab' });
      const active = page.locator('.project-aow-surface:not([hidden])');
      await active.getByText('Retained reply for main', { exact: true }).waitFor();
      await active.locator('.project-aow-inline-error').filter({ hasText: 'Terminal inventory unavailable' }).waitFor();
      if (selection === 'worktree') await page.locator('.project-aow-pinned button[title="/workspace/linked"]').click();
      else await active.getByRole('tab', { name: /^Saved ACP second/ }).click();
      const selected = selection === 'worktree' ? linked : second;
      await page.waitForURL(url => url.pathname === `/aow/tabs/acp/${selected.thread_id}`);
      await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).fill('New selection wins');
      state.terminalUnavailable = false;
      await refreshOnActivation(page, 'visibilitychange');
      await active.getByRole('tab', { name: selection === 'worktree' ? /^Other Tab/ : /^Main Tab/ }).waitFor();
      await refreshOnActivation(page, 'focus');
      assert.equal(await page.locator('.project-aow-worktrees button.active').getAttribute('title'), selected.cwd);
      assert.equal(new URL(page.url()).pathname, `/aow/tabs/acp/${selected.thread_id}`);
      assert.equal(await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).innerText(), 'New selection wins');
      assert.deepEqual(state.acpWrites, []);
    });
  }

  for (const selection of ['worktree', 'tab']) {
    await test(`explicit ${selection} selection cancels a delayed cross-worktree ACP lookup`, async t => {
      const main = { ...acpSession('main', '/workspace/main'), status: 'idle' };
      const second = { ...acpSession('second', '/workspace/main'), status: 'idle' };
      const linked = { ...acpSession('linked'), status: 'idle' };
      const { page, state } = await fixture(t, { acp: true, acpHistory: [main, second, linked], restoreAcpSessions: true,
        tabUrl: '/aow/tabs/acp/main' });
      const active = page.locator('.project-aow-surface:not([hidden])');
      await active.getByText('Retained reply for main', { exact: true }).waitFor();
      const started = Promise.withResolvers();
      const release = Promise.withResolvers();
      const finished = Promise.withResolvers();
      t.after(() => release.resolve());
      await page.route('**/api/acp/sessions/linked/metadata', async route => {
        started.resolve();
        await release.promise;
        await route.fulfill({ json: linked });
        finished.resolve();
      });
      await active.getByRole('button', { name: '打开 ACP 会话 Saved ACP linked', exact: true }).click();
      await started.promise;
      // Even reselecting the current worktree must supersede the pending link.
      if (selection === 'worktree') await page.locator('.project-aow-worktrees button[title="/workspace/main"]').click();
      else await active.getByRole('tab', { name: /^Saved ACP second/ }).click();
      const selected = selection === 'worktree' ? main : second;
      await page.waitForURL(url => url.pathname === `/aow/tabs/acp/${selected.thread_id}`);
      release.resolve();
      await finished.promise;
      await refreshOnActivation(page, 'focus');
      assert.equal(await page.locator('.project-aow-worktrees button.active').getAttribute('title'), main.cwd);
      assert.equal(new URL(page.url()).pathname, `/aow/tabs/acp/${selected.thread_id}`);
      await active.getByText(`Retained reply for ${selected.thread_id}`, { exact: true }).waitFor();
      assert.deepEqual(state.acpWrites, []);
    });
  }

  for (const mounted of [true, false]) {
    await test(`ACP cross-worktree deletion cleans ${mounted ? 'mounted' : 'unmounted'} owner tabs and preserves other sessions`, async t => {
      const deleted = { ...acpSession('delete-linked'), status: 'idle' };
      const kept = { ...acpSession('keep-linked'), status: 'idle' };
      const main = { ...acpSession('keep-main', '/workspace/main'), status: 'idle' };
      const saved = [deleted, kept].map(session => ({ id: `new:${session.thread_id}`, thread_id: session.thread_id, agentId: 'test', title: session.title }));
      const { page, state } = await fixture(t, { acp: true, acpHistory: [deleted, kept, main], acpSavedTabs: saved,
        tabUrl: '/aow/tabs/acp/delete-linked' });
      const active = page.locator('.project-aow-surface:not([hidden])');
      await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).fill('Discard deleted draft');
      await active.getByRole('tab', { name: /^Saved ACP keep-linked/ }).click();
      await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).fill('Keep owner draft');
      if (mounted) await page.locator('.project-aow-worktrees button[title="/workspace/main"]').click();
      else await page.goto(`${base}/aow/tabs/terminal/main-tab?ui=desktop`);
      await page.getByRole('button', { name: 'ACP 面板', exact: true }).click();
      const list = active.getByRole('region', { name: 'ACP 会话列表', exact: true });
      await list.getByRole('button', { name: '打开 ACP 会话 Saved ACP keep-main', exact: true }).click();
      await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).fill('Keep current draft');
      const remove = async () => {
        await list.getByRole('button', { name: '删除 ACP 会话 Saved ACP delete-linked', exact: true }).click();
        await page.getByRole('alertdialog').getByRole('button', { name: '删除', exact: true }).click();
      };
      state.acpDeleteError = 'Deletion unavailable';
      await remove();
      await list.getByRole('alert').filter({ hasText: 'Deletion unavailable' }).waitFor();
      assert.deepEqual(await page.evaluate(() => JSON.parse(localStorage.getItem('aow-acp-tabs:/workspace/linked')).map(tab => tab.id)), saved.map(tab => tab.id));
      state.acpDeleteError = '';
      await remove();
      await page.waitForFunction(() => JSON.parse(localStorage.getItem('aow-acp-tabs:/workspace/linked')).every(tab => tab.thread_id !== 'delete-linked'));
      assert.deepEqual(await page.evaluate(() => JSON.parse(localStorage.getItem('aow-acp-tabs:/workspace/linked')).map(tab => tab.id)), ['new:keep-linked']);
      assert.deepEqual(await page.evaluate(() => JSON.parse(localStorage.getItem('aow-acp-tabs:/workspace/main')).map(tab => tab.thread_id)), ['keep-main']);
      assert.equal(await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).innerText(), 'Keep current draft');
      assert.equal(state.acpSessions.some(session => session.thread_id === deleted.thread_id), false);
      await page.locator('.project-aow-worktrees button[title="/workspace/linked"]').click();
      await active.getByRole('tab', { name: /^Saved ACP keep-linked/ }).click();
      assert.equal(await active.getByRole('tab', { name: /^Saved ACP delete-linked/ }).count(), 0, 'mounted owner state cannot resurrect the deleted tab');
      if (mounted) assert.equal(await active.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true }).innerText(), 'Keep owner draft');
      await page.reload();
      await active.getByRole('tab', { name: /^Saved ACP keep-linked/ }).waitFor();
      assert.equal(await active.getByRole('tab', { name: /^Saved ACP delete-linked/ }).count(), 0, 'restoration cannot resurrect the deleted tab');
    });
  }

  await test('mobile ACP navigation lists workspace sessions and opens the shared conversation page', async t => {
    const older = { ...acpSession('older'), title: '检查项目配置', agent_id: 'codex-acp', updated_at: '2026-10-13T09:00:00Z' };
    const newer = { ...acpSession('newer'), title: '优化手机端会话导航', agent_id: 'claude-agent-acp', updated_at: '2026-10-13T10:00:00Z' };
    const { page, state } = await fixture(t, { acp: true, mobile: true, acpHistory: [older, acpSession('elsewhere', '/workspace/main'), newer] });
    const navigation = page.getByRole('navigation', { name: '工作区导航' });
    const entry = navigation.getByRole('button', { name: 'ACP', exact: true });
    await entry.click();
    const list = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    await list.getByRole('button', { name: '打开 ACP 会话 检查项目配置', exact: true }).waitFor();
    assert.equal(await entry.getAttribute('aria-current'), 'page');
    assert.deepEqual(await list.locator('.mobile-list-row strong').allTextContents(), [newer.title, older.title]);
    assert.equal(await list.locator('.mobile-row-icon .agent-icon').count(), 2);
    assert.deepEqual(state.acpWrites, [], 'browsing history does not create, resume or prompt an agent');
    assert.equal(state.acpConnections, 0);
    assert.equal(state.reads.some(path => /^\/api\/acp\/sessions\/[^/]+$/.test(path)), false, 'the list only reads session metadata');
    for (const width of [390, 320]) {
      await page.setViewportSize({ width, height: 844 });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
      for (const button of await navigation.getByRole('button').all()) {
        const box = await button.boundingBox();
        assert.ok(box.x >= 0 && box.x + box.width <= width && box.width >= 44, 'all seven navigation buttons remain reachable');
      }
      await page.screenshot({ path: join(screenshotDir, `aow-mobile-acp-list-${width}.png`) });
    }
    await page.setViewportSize({ width: 390, height: 844 });
    await list.getByRole('button', { name: '打开 ACP 会话 检查项目配置', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/older');
    await page.getByText('Retained reply for older', { exact: true }).waitFor();
    const editor = page.getByLabel('ACP 消息', { exact: true });
    await editor.fill('保留手机端草稿');
    await page.screenshot({ path: join(screenshotDir, 'aow-mobile-acp-conversation.png') });
    await page.getByRole('button', { name: '返回 ACP 会话列表', exact: true }).click();
    await list.waitFor();
    await list.getByRole('button', { name: '打开 ACP 会话 检查项目配置', exact: true }).click();
    await editor.waitFor();
    assert.equal(await editor.innerText(), '保留手机端草稿');
    await page.reload();
    await page.getByText('Retained reply for older', { exact: true }).waitFor();
    assert.equal(await entry.getAttribute('aria-current'), 'page');
    await entry.click();
    await list.waitFor();
    await list.getByRole('button', { name: '打开 ACP 会话 优化手机端会话导航', exact: true }).click();
    await page.getByText('Retained reply for newer', { exact: true }).waitFor();
    await page.goBack();
    await list.waitFor();
    await page.reload();
    await list.getByRole('button', { name: '打开 ACP 会话 检查项目配置', exact: true }).waitFor();
    assert.equal(await list.locator('.mobile-list-row').count(), 2);
    assert.deepEqual(state.acpWrites, ['POST /api/acp/sessions/older/actions', 'POST /api/acp/sessions/newer/actions'], 'opening restores existing sessions once without creating duplicates');
    assert.equal(state.acpSessions.length, 3);
  });

  await test('mobile ACP lists handle empty workspaces, refresh failures and workspace changes', async t => {
    const { page, state, root } = await fixture(t, { acp: true, mobile: true });
    await page.getByRole('navigation', { name: '工作区导航' }).getByRole('button', { name: 'ACP', exact: true }).click();
    const list = page.getByRole('region', { name: 'ACP 会话列表', exact: true });
    await list.getByText('这个工作区还没有 ACP 会话记录。', { exact: true }).waitFor();
    state.acpListError = '读取 ACP 会话失败';
    await list.getByRole('button', { name: '刷新', exact: true }).click();
    await list.getByRole('alert').getByText(state.acpListError, { exact: true }).waitFor();
    state.acpListError = '';
    state.acpSessions.push(acpSession('linked', root), acpSession('main', '/workspace/main'));
    await list.getByRole('button', { name: '重新加载', exact: true }).click();
    await list.getByRole('button', { name: '打开 ACP 会话 Saved ACP linked', exact: true }).waitFor();
    assert.equal(await list.getByRole('alert').count(), 0);
    await page.goto(`${base}/aow?ui=mobile#workspace=%2Fworkspace%2Fmain&view=acp`);
    await list.getByRole('button', { name: '打开 ACP 会话 Saved ACP main', exact: true }).waitFor();
    assert.equal(await list.locator('.mobile-list-row').count(), 1);
    await page.getByRole('navigation', { name: '工作区导航' }).getByRole('button', { name: '终端', exact: true }).click();
    state.acpSessions.find(session => session.thread_id === 'main').title = 'Updated from desktop';
    await page.getByRole('navigation', { name: '工作区导航' }).getByRole('button', { name: 'ACP', exact: true }).click();
    await list.getByRole('button', { name: '打开 ACP 会话 Updated from desktop', exact: true }).waitFor();
    assert.deepEqual(state.acpWrites, []);
    assert.equal(state.acpConnections, 0);
  });

  await test('ACP URL navigation reuses open tabs and preserves their independent drafts', async t => {
    const first = { ...acpSession('one'), status: 'idle' };
    const second = { ...acpSession('two'), status: 'idle' };
    const { page, state } = await fixture(t, { acp: true, acpHistory: [first, second], tabUrl: '/aow/tabs/acp/one?workspace=wt-1' });
    const composer = page.locator('.acp-tab-host:visible').getByLabel('ACP 消息', { exact: true });
    await composer.fill('First draft');
    await page.evaluate(() => { history.pushState({}, '', '/aow/tabs/acp/two?workspace=wt-1&ui=desktop'); window.dispatchEvent(new PopStateEvent('popstate')); });
    await page.getByText('Retained reply for two', { exact: true }).waitFor();
    await composer.fill('Second draft');
    await page.goBack();
    await page.getByText('Retained reply for one', { exact: true }).waitFor();
    assert.equal(await composer.innerText(), 'First draft');
    await page.goForward();
    await page.getByText('Retained reply for two', { exact: true }).waitFor();
    assert.equal(await composer.innerText(), 'Second draft');
    assert.equal(await page.getByRole('tab', { name: /^Saved ACP one/ }).count(), 1);
    assert.equal(await page.getByRole('tab', { name: /^Saved ACP two/ }).count(), 1);
    assert.deepEqual(state.acpWrites, []);
    assert.deepEqual(state.errors, []);
  });

  await test('ACP links ignore obsolete tab records and reuse a floating draft view across reload', async t => {
    const session = acpSession();
    const viewId = 'new:existing-draft';
    const { page, state } = await fixture(t, { acp: true, floating: true, preserveFloating: true, acpHistory: [session],
      acpSavedTabs: [
        { id: 'old-view', sessionId: 'old-thread', agentId: session.agent_id, title: 'Obsolete tab' },
        { id: viewId, thread_id: session.thread_id, agentId: session.agent_id, title: session.title },
      ],
      extraFloatingTabs: [{ workspace: session.cwd, id: `acp:${viewId}`, targetId: viewId, kind: 'acp', label: session.title }],
      tabUrl: '/aow/tabs/acp/acp-local' });
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await floating.getByText('Retained reply for acp-local', { exact: true }).waitFor();
    await floating.getByLabel('ACP 消息', { exact: true }).waitFor();
    await page.reload();
    await floating.getByText('Retained reply for acp-local', { exact: true }).waitFor();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local');
    const tabs = await page.evaluate(() => JSON.parse(localStorage.getItem('aow-acp-tabs:/workspace/linked')));
    assert.equal(tabs.length, 1); assert.equal(tabs[0].id, viewId);
    assert.deepEqual(state.acpWrites, ['POST /api/acp/sessions/acp-local/actions']);
    assert.deepEqual(state.errors, []);
  });

  await test('missing ACP threads and removed workspaces fail without starting an agent', async t => {
    for (const missing of [true, false]) {
      const { page, state } = await fixture(t, { acp: true, acpHistory: [acpSession('acp-local', missing ? '/workspace/linked' : '/removed')], tabUrl: '/aow/tabs/acp/' + (missing ? 'missing' : 'acp-local') });
      await page.getByRole('alert').filter({ hasText: missing ? '该 ACP 会话已不存在。' : '目标 Tab 所属工作区不存在或已被移除。' }).waitFor();
      assert.deepEqual(state.acpWrites, []);
      assert.equal(state.acpConnections, 0);
    }
  });

  await test('legacy ACP links derive workspace from thread metadata and discard the old query', async t => {
    const { page, state } = await fixture(t, { acp: true, acpHistory: [acpSession()], tabUrl: '/aow/tabs/acp/acp-local?workspace=wt-0' });
    await page.getByText('Retained reply for acp-local', { exact: true }).waitFor();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local' && !url.searchParams.has('workspace'));
    assert.deepEqual(state.errors, []);
  });

  await test('Inbox opens in the floating workspace, survives refresh and leaves existing terminals intact', async t => {
    const { page, state } = await fixture(t, { floating: true, preserveFloating: true });
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await floating.getByRole('tab', { name: /Target Tab/ }).waitFor();
    await floating.getByRole('button', { name: "收件箱", exact: true }).click();
    const panel = floating.getByRole('region', { name: "收件箱", exact: true });
    await panel.getByRole('button', { name: '添加需求', exact: true }).click();
    await fillInboxEditor(panel.getByRole('textbox', { name: '新需求 Markdown' }), '全局需求，无需项目');
    await assertInboxAutosave(panel);
    await panel.getByRole('textbox', { name: '新需求 Markdown' }).press('Meta+Enter');
    const row = panel.getByRole('article', { name: '全局需求，无需项目' });
    await row.waitFor();
    const before = await row.locator('.inbox-row-meta').boundingBox();
    await row.locator('.inbox-edit-target').dblclick();
    assert.equal((await row.locator('.inbox-row-meta').boundingBox()).height, before.height);
    await assertInboxAutosave(row);
    await page.screenshot({ path: join(screenshotDir, 'inbox-desktop-autosave.png') });
    await row.getByRole('textbox', { name: '需求 Markdown', exact: true }).press('Escape');
    await floating.getByRole('button', { name: '固定显示浮动工作区', exact: true }).click();
    await row.getByRole('button', { name: '选择标签', exact: true }).click();
    const labels = page.getByRole('dialog', { name: '选择需求标签', exact: true });
    await labels.waitFor();
    await page.mouse.move(1, 1);
    await page.waitForTimeout(300);
    assert.equal(await floating.isVisible(), true, 'a portaled label menu keeps the floating workspace open');
    assert.ok(!page.url().includes('/aow/tabs/'), 'menu focus remains in the Inbox workspace');
    await page.mouse.move(before.x + 10, before.y + 10);
    await page.keyboard.press('Escape');
    assert.equal(await labels.count(), 0);
    assert.equal(state.inbox[0].project_id, null);
    await page.reload();
    await floating.getByRole('region', { name: "收件箱", exact: true }).getByRole('article', { name: '全局需求，无需项目' }).waitFor();
    await floating.getByRole('tab', { name: /Target Tab/ }).click();
    await floating.locator('.floating-workspace-host:not([hidden]) .terminal-pane').waitFor();
    assert.deepEqual(state.errors, []); assert.deepEqual(state.mutations, []);
  });
  await test('Inbox mobile entry captures globally, restores after refresh and returns to projects', async t => {
    const { page, state } = await fixture(t, { mobile: true, entry: '' });
    await page.getByRole('button', { name: "收件箱", exact: true }).click();
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    await fillInboxEditor(page.getByRole('textbox', { name: '新需求 Markdown' }), '手机上的想法');
    await assertInboxAutosave(page.locator('.inbox-panel'));
    await page.getByRole('textbox', { name: '新需求 Markdown' }).press('Control+Enter');
    const row = page.getByRole('article', { name: '手机上的想法' });
    await row.waitFor();
    const before = await row.locator('.inbox-row-meta').boundingBox();
    await row.locator('.inbox-edit-target').dblclick();
    assert.equal((await row.locator('.inbox-row-meta').boundingBox()).height, before.height);
    await assertInboxAutosave(row);
    await page.screenshot({ path: join(screenshotDir, 'inbox-mobile-autosave.png') });
    await row.getByRole('textbox', { name: '需求 Markdown', exact: true }).press('Escape');
    await page.reload();
    await page.getByRole('article', { name: '手机上的想法' }).waitFor();
    await page.getByRole('button', { name: '返回项目列表', exact: true }).click();
    await page.getByRole('heading', { name: '你的项目', exact: true }).waitFor();
    assert.equal(state.inbox[0].project_id, null);
  });

  const desktopTarget = page => page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab.active').filter({ hasText: 'Target Tab' });
  await test('deep link survives login and refresh and overrides the saved project and tab', async t => {
    const { page, state, root } = await fixture(t, { login: true });
    await page.getByLabel('账号').waitFor();
    assert.equal(state.targetReads, 0);
    await page.getByLabel('账号').fill('admin');
    await page.getByLabel('密码', { exact: true }).fill('test-password');
    await page.getByRole('button', { name: '登录', exact: true }).click();
    await desktopTarget(page).waitFor();
    assert.equal(await page.locator('.project-aow-status').innerText().then(value => value.includes(root)), true);
    assert.match(page.url(), /\/aow\/tabs\/terminal\/target\?/);
    await page.reload();
    await desktopTarget(page).waitFor();
    await page.screenshot({ path: join(screenshotDir, 'aow-tab-link-desktop.png') });
    assert.ok(state.targetReads >= 2);
  });

  await test('deep link reveals a minimized floating tab and also supports the global workspace', async t => {
    for (const global of [false, true]) {
      const { page } = await fixture(t, { floating: !global, global });
      await page.locator('[data-floating-workspace] .project-aow-center-tab.active').filter({ hasText: 'Target Tab' }).waitFor();
      await page.locator('[data-floating-workspace] .terminal-emulator-shell:not(.restore-pending)').first().waitFor();
    }
  });

  await test('unsupported saved tab types are discarded while valid floating tabs restore', async t => {
    const { page } = await fixture(t, { floating: true, extraFloatingTabs: [
      { workspace: '/workspace/linked', id: 'retired:42', targetId: '42', kind: 'retired', label: 'Expired Tab' },
    ] });
    const active = page.locator('[data-floating-workspace] .project-aow-center-tab.active').filter({ hasText: 'Target Tab' });
    await active.waitFor();
    const saved = await page.evaluate(() => JSON.parse(localStorage.getItem('aow-floating-tabs-v1')));
    assert.deepEqual(saved.map(tab => tab.kind), ['terminal']);
    assert.deepEqual(saved.map(tab => tab.id), ['terminal:target']);
    await page.reload();
    await active.waitFor();
  });

  await test('mobile link selects the target terminal including global tabs and survives refresh', async t => {
    for (const global of [false, true]) {
      const { page } = await fixture(t, { mobile: true, global });
      await page.locator('.mobile-terminal-tabs [aria-selected="true"]').filter({ hasText: 'Target Tab' }).waitFor();
      await page.reload();
      await page.locator('.mobile-terminal-tabs [aria-selected="true"]').filter({ hasText: 'Target Tab' }).waitFor();
    }
  });

  await test('temporary Inbox terminal links open without a registered Worktree and survive reload', async t => {
    const { page } = await fixture(t, { temporary: true });
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await floating.locator('.terminal-pane').waitFor();
    await page.waitForURL(url => url.pathname === '/aow/tabs/terminal/target');
    assert.equal(await floating.isVisible(), true);
    await page.reload();
    await floating.locator('.terminal-pane').waitFor();
    assert.equal(await floating.isVisible(), true);
  });

  await test('deleted tabs and removed workspaces show errors without creating a replacement', async t => {
    const { page } = await fixture(t, { missing: true });
    await page.getByRole('alert').filter({ hasText: '该 Tab 已关闭或不存在。' }).waitFor();
    assert.equal(await page.getByRole('link', { name: '返回工作台' }).getAttribute('href'), '/aow/?ui=desktop');
    for (const mobile of [false, true]) {
      const removed = await fixture(t, { removedWorkspace: true, mobile });
      await removed.page.getByText('目标 Tab 所属工作区不存在或已被移除。', { exact: true }).waitFor();
      await removed.page.getByRole('link', { name: '返回工作台' }).click();
      await removed.page.waitForURL(url => url.pathname === '/aow/');
    }
  });

  const expectPath = (page, id) => page.waitForURL(url => url.pathname === (id ? `/aow/tabs/terminal/${id}` : '/aow/'));
  const desktopTab = (page, name) => page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab').filter({ hasText: name });
  const terminalRow = (page, id) => page.locator(`.project-aow-surface:not([hidden]) .terminal-family[data-tab-id="${id}"] > .terminal-family-root > .terminal-panel-row`);
  async function expectTerminalCompletion(page, id, completed) {
    const row = terminalRow(page, id);
    await row.waitFor();
    await row.getByRole('img', { name: '任务已完成，点击查看', exact: true }).waitFor({ state: completed ? 'visible' : 'detached' });
  }
  async function notifyTab(page, tabId = 'target', root = worktrees[1].path, extra = {}) {
    await page.waitForFunction(() => window.taskStopReady());
    await page.evaluate(({ tabId, root, extra }) => window.sendTaskStop({ agent: 'codex', session_id: 'notification-session',
      title: 'Notification task', cwd: root,
      sources: [{ tab_id: tabId, tab_name: tabId, project_name: 'Link Fixture', workspace_root: root }],
      ...extra,
    }), { tabId, root, extra });
    return page.getByRole('button', { name: `打开通知：Link Fixture · ${tabId}`, exact: true }).last();
  }

  async function expectUnreadCount(page, root, count, pinned = false) {
    const row = page.locator(`.project-aow-${pinned ? 'pinned' : 'worktrees'} button[title="${root}"]`);
    const badge = row.locator('.project-aow-worktree-unread');
    if (!count) await badge.waitFor({ state: 'detached' });
    else {
      await badge.and(page.getByLabel(`${count} 条未读通知`, { exact: true })).waitFor();
      assert.equal(await badge.textContent(), String(count));
    }
  }

  await test('the status bar reminder icon opens a resizable panel beside operation logs without a count', async t => {
    const { page } = await fixture(t, { entry: '' });
    await expectPath(page, 'main-tab');
    await page.route('**/api/operation-logs**', route => route.fulfill({ json: { items: [], next_cursor: null, budget_exhausted: false, scanned_bytes: 0 } }));
    const button = page.locator('.project-aow-status').getByRole('button', { name: '提醒', exact: true });
    const panel = page.getByRole('region', { name: '提醒', exact: true });
    const logs = page.getByRole('region', { name: '操作日志', exact: true });
    assert.equal(await button.textContent(), '');
    assert.equal(await button.locator('svg').count(), 1);
    const logBox = await page.getByRole('button', { name: '操作日志', exact: true }).boundingBox();
    const reminderBox = await button.boundingBox();
    assert.ok(reminderBox.x >= logBox.x + logBox.width && reminderBox.x - logBox.x - logBox.width <= 8);
    await button.click();
    await panel.getByText('暂无待处理提醒', { exact: true }).waitFor();
    assert.equal(await panel.getByRole('button', { name: '清空全部', exact: true }).isDisabled(), true);
    assert.equal(await button.getAttribute('aria-expanded'), 'true');
    const original = await panel.boundingBox();
    const resize = panel.getByRole('separator', { name: '调整提醒面板高度', exact: true });
    await resize.press('ArrowUp');
    assert.equal((await panel.boundingBox()).height, original.height + 20);
    await panel.press('Escape');
    await panel.waitFor({ state: 'detached' });
    assert.equal(await button.evaluate(element => document.activeElement === element), true);
    await button.press('Enter');
    await panel.waitFor();
    await page.getByRole('button', { name: '操作日志', exact: true }).click();
    await logs.waitFor();
    await panel.waitFor({ state: 'detached' });
    await button.click();
    await panel.waitFor();
    await logs.waitFor({ state: 'detached' });
    await button.click();
    await panel.waitFor({ state: 'detached' });
  });

  await test('the reminder panel recovers hidden notices, opens their sources and permanently clears orphaned reminders', async t => {
    const mainRoot = worktrees[0].path;
    const { page, root } = await fixture(t, { entry: '', pinned: [mainRoot], acp: true, acpHistory: [acpSession()] });
    await expectPath(page, 'main-tab');
    await (await notifyTab(page, 'deleted-tab', mainRoot)).waitFor();
    await (await notifyTab(page, 'target', root)).waitFor();
    await (await notifyTab(page, 'ACP task', root, { agent: 'test', sources: [{ tab_id: '', thread_id: 'acp-local', tab_name: 'ACP task', project_name: 'Link Fixture', workspace_root: root }] })).waitFor();
    await notifyTab(page, '', root, { title: 'Legacy reminder', sources: [] });
    await page.getByRole('button', { name: '全部关闭', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await page.reload();
    await expectPath(page, 'main-tab');
    for (const pinned of [false, true]) await expectUnreadCount(page, mainRoot, 1, pinned);
    await expectUnreadCount(page, root, 2);

    const button = page.locator('.project-aow-status').getByRole('button', { name: '提醒', exact: true });
    const panel = page.getByRole('region', { name: '提醒', exact: true });
    assert.equal(await button.textContent(), '', 'the icon stays count-free even with unread notices');
    await button.click();
    await panel.getByText('4 条待处理提醒 · 包含已关闭弹框的提醒', { exact: true }).waitFor();
    assert.equal(await page.locator('.agent-task-notice').count(), 0, 'opening the panel does not reopen popups');
    assert.equal(await panel.locator('article').count(), 4);
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/terminal/main-tab');
    await panel.getByRole('button', { name: '打开提醒：Link Fixture · deleted-tab', exact: true }).click();
    await panel.getByRole('alert').filter({ hasText: '该 Tab 已关闭或不存在。' }).waitFor();
    for (const pinned of [false, true]) await expectUnreadCount(page, mainRoot, 1, pinned);
    await page.screenshot({ path: join(screenshotDir, 'aow-reminder-panel.png') });
    await panel.getByRole('button', { name: '移除提醒：Link Fixture · deleted-tab', exact: true }).click();
    for (const pinned of [false, true]) await expectUnreadCount(page, mainRoot, 0, pinned);

    await panel.getByRole('button', { name: '打开提醒：Link Fixture · target', exact: true }).click();
    await expectPath(page, 'target');
    await panel.getByText('2 条待处理提醒 · 包含已关闭弹框的提醒', { exact: true }).waitFor();
    await expectUnreadCount(page, root, 1);
    await panel.getByRole('button', { name: '打开提醒：Link Fixture · ACP task', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local');
    await panel.getByText('1 条待处理提醒 · 包含已关闭弹框的提醒', { exact: true }).waitFor();
    await expectUnreadCount(page, root, 0);

    await (await notifyTab(page, 'other', root)).waitFor();
    await (await notifyTab(page, 'deleted-tab', mainRoot)).waitFor();
    await panel.getByText('3 条待处理提醒 · 包含已关闭弹框的提醒', { exact: true }).waitFor();
    await panel.getByRole('button', { name: '清空全部', exact: true }).click();
    await panel.getByText('暂无待处理提醒', { exact: true }).waitFor();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await expectUnreadCount(page, root, 0);
    for (const pinned of [false, true]) await expectUnreadCount(page, mainRoot, 0, pinned);
    await page.reload();
    await page.getByText('Retained reply for acp-local', { exact: true }).waitFor();
    await button.click();
    await panel.getByText('暂无待处理提醒', { exact: true }).waitFor();
    await expectUnreadCount(page, root, 0);
    for (const pinned of [false, true]) await expectUnreadCount(page, mainRoot, 0, pinned);
    assert.equal(await page.locator('.agent-task-notice').count(), 0);
  });

  await test('ACP completion notifications open the original conversation and acknowledge its turns on desktop and mobile', async t => {
    for (const mobile of [false, true]) await t.test(mobile ? 'mobile' : 'desktop', async t => {
      const { page, root, state } = await fixture(t, { acp: true, mobile, acpHistory: [acpSession()] });
      await expectPath(page, 'target');
      const extra = { agent: 'test', sources: [{ tab_id: '', thread_id: 'acp-local', tab_name: 'ACP task', project_name: 'Link Fixture', workspace_root: root }] };
      await (await notifyTab(page, 'ACP task', root, extra)).waitFor();
      if (!mobile) await expectUnreadCount(page, root, 1);
      await (await notifyTab(page, 'ACP task', root, extra)).click();
      await page.getByText('Retained reply for acp-local', { exact: true }).waitFor();
      await page.waitForURL(url => url.pathname === '/aow/tabs/acp/acp-local');
      await page.locator('.agent-task-notice').waitFor({ state: 'detached' });
      if (!mobile) await expectUnreadCount(page, root, 0);
      await notifyTab(page, 'ACP task', root, extra);
      await (await notifyTab(page, 'target', root)).waitFor();
      assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · ACP task', exact: true }).count(), 0);
      await page.reload();
      await page.getByText('Retained reply for acp-local', { exact: true }).waitFor();
      assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · ACP task', exact: true }).count(), 0);
      assert.equal(state.acpWrites.filter(value => value === 'POST /api/acp/sessions').length, 0);
    });
  });

  await test('worktree unread badges stay in sync in Projects and Pinned through reload and tab activation', async t => {
    const { page, root } = await fixture(t, { entry: '', pinned: [worktrees[1].path] });
    await expectPath(page, 'main-tab');
    await page.locator('.project-aow-pinned button').waitFor();
    for (const pinned of [false, true]) await expectUnreadCount(page, root, 0, pinned);

    // Multiple sources in the same worktree count as one notification.
    await notifyTab(page, 'target', root, { sources: ['target', 'deleted-tab'].map(tab_id => ({
      tab_id, tab_name: tab_id, project_name: 'Link Fixture', workspace_root: root,
    })) });
    await notifyTab(page, 'other', root);
    for (const pinned of [false, true]) await expectUnreadCount(page, root, 2, pinned);
    await expectUnreadCount(page, worktrees[0].path, 0);
    await page.evaluate(() => localStorage.setItem('aow-left-width', '180'));
    await page.reload();
    await expectPath(page, 'main-tab');
    for (const pinned of [false, true]) {
      await expectUnreadCount(page, root, 2, pinned);
      const row = page.locator(`.project-aow-${pinned ? 'pinned' : 'worktrees'} button[title="${root}"]`);
      const rowBox = await row.boundingBox();
      const badgeBox = await row.locator('.project-aow-worktree-unread').boundingBox();
      const rightInset = await row.evaluate(element => {
        const style = getComputedStyle(element);
        return parseFloat(style.paddingRight) + parseFloat(style.borderRightWidth);
      });
      assert.ok(badgeBox.x > rowBox.x + rowBox.width / 2);
      assert.ok(Math.abs(rowBox.x + rowBox.width - badgeBox.x - badgeBox.width - rightInset) <= 1);
      assert.ok(badgeBox.y >= rowBox.y && badgeBox.y + badgeBox.height <= rowBox.y + rowBox.height);
    }
    await page.screenshot({ path: join(screenshotDir, 'aow-worktree-unread.png') });

    // Opening the worktree activates Other Tab and clears only its notification.
    await page.locator(`.project-aow-pinned button[title="${root}"]`).click();
    await expectPath(page, 'other');
    for (const pinned of [false, true]) await expectUnreadCount(page, root, 1, pinned);
    await (await notifyTab(page, 'main-tab', worktrees[0].path)).waitFor();
    await desktopTab(page, 'Target Tab').click();
    await expectPath(page, 'target');
    for (const pinned of [false, true]) await expectUnreadCount(page, root, 0, pinned);
    await expectUnreadCount(page, worktrees[0].path, 1);

    await (await notifyTab(page, 'other', root)).waitFor();
    await page.getByRole('button', { name: '全部关闭', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    for (const pinned of [false, true]) await expectUnreadCount(page, root, 1, pinned);
    await expectUnreadCount(page, worktrees[0].path, 1);
  });

  await test('closing popups preserves worktree badges and terminal completion icons until their source tabs activate', async t => {
    const { page, root } = await fixture(t, { entry: '', pinned: [worktrees[1].path] });
    await expectPath(page, 'main-tab');
    const expectCounts = async count => {
      for (const pinned of [false, true]) await expectUnreadCount(page, root, count, pinned);
    };
    await (await notifyTab(page, 'target', root)).waitFor();
    await (await notifyTab(page, 'other', root)).waitFor();
    await expectCounts(2);
    await expectTerminalCompletion(page, 'target', true);
    await expectTerminalCompletion(page, 'other', true);
    await expectTerminalCompletion(page, 'main-tab', false);
    await page.getByRole('button', { name: '关闭通知：Link Fixture · target', exact: true }).click();
    await page.getByRole('button', { name: '打开通知：Link Fixture · target', exact: true }).waitFor({ state: 'detached' });
    await expectCounts(2);
    await expectTerminalCompletion(page, 'target', true);
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/terminal/main-tab');
    assert.equal(await page.locator('.agent-task-notice-tab').textContent(), 'other');

    await page.reload();
    await expectPath(page, 'main-tab');
    await expectCounts(2);
    await expectTerminalCompletion(page, 'target', true);
    await expectTerminalCompletion(page, 'other', true);
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(), ['other']);
    // A later turn in the same tab gets a new popup and a new unread entry.
    await (await notifyTab(page, 'target', root)).waitFor();
    await expectCounts(3);
    await page.getByText('2 条待处理', { exact: true }).waitFor();
    await page.screenshot({ path: join(screenshotDir, 'aow-notification-close.png') });
    await page.getByRole('button', { name: '全部关闭', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await expectCounts(3);
    await page.reload();
    await expectPath(page, 'main-tab');
    await expectCounts(3);
    assert.equal(await page.locator('.agent-task-notice').count(), 0);
    await expectTerminalCompletion(page, 'target', true);
    await expectTerminalCompletion(page, 'other', true);
    const row = terminalRow(page, 'target');
    const icon = row.getByRole('img', { name: '任务已完成，点击查看', exact: true });
    const iconBox = await icon.boundingBox();
    const menuBox = await row.getByRole('button', { name: 'Target Tab 终端操作', exact: true }).boundingBox();
    assert.equal(iconBox.width, 14);
    assert.equal(iconBox.height, 14);
    assert.equal(await icon.locator('svg').evaluate(element => getComputedStyle(element).color), 'rgb(115, 201, 145)');
    assert.ok(iconBox.x + iconBox.width < menuBox.x);
    assert.ok(Math.abs(iconBox.y + iconBox.height / 2 - menuBox.y - menuBox.height / 2) <= 1);
    assert.equal(await icon.getAttribute('title'), '任务已完成，点击查看');
    await page.screenshot({ path: join(screenshotDir, 'aow-terminal-completion.png') });

    await page.locator(`.project-aow-pinned button[title="${root}"]`).click();
    await expectPath(page, 'other');
    await expectCounts(2);
    await expectTerminalCompletion(page, 'other', false);
    await expectTerminalCompletion(page, 'target', true);
    await terminalRow(page, 'target').locator('.terminal-panel-open').click();
    await expectPath(page, 'target');
    await expectCounts(0);
    await expectTerminalCompletion(page, 'target', false);
    await page.reload();
    await expectPath(page, 'target');
    await expectCounts(0);
    await expectTerminalCompletion(page, 'target', false);
    assert.equal(await page.locator('.agent-task-notice').count(), 0);
  });

  await test('completion notifications activate existing tabs in place across workspaces, floating areas and mobile', async t => {
    for (const [name, options] of [['another workspace', {}], ['same workspace', { sameWorkspace: true }],
      ['floating tab', { floating: true }], ['global tab', { global: true }],
      ['mobile', { mobile: true }], ['mobile global tab', { mobile: true, global: true }]]) await t.test(name, async t => {
      const { page, root } = await fixture(t, { ...options, entry: options.sameWorkspace ? 'other' : '' });
      if (options.mobile) await page.getByRole('heading', { name: '你的项目' }).waitFor();
      else await expectPath(page, options.sameWorkspace ? 'other' : 'main-tab');
      await page.evaluate(() => {
        window.routeTestMarker = true;
        window.previousWorkspace = document.querySelector('.project-aow-surface:not([hidden])');
      });
      const depth = await page.evaluate(() => history.length);
      const card = await notifyTab(page, 'target', root);
      await card.click({ position: { x: 5, y: 5 } });
      await expectPath(page, 'target');
      const active = options.mobile ? page.locator('.mobile-terminal-tabs [aria-selected="true"]')
        : options.floating || options.global ? page.locator('[data-floating-workspace] .project-aow-center-tab.active')
        : page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab.active');
      await active.filter({ hasText: 'Target Tab' }).waitFor();
      assert.equal(await page.evaluate(() => window.routeTestMarker), true);
      assert.equal(page.context().pages().length, 1);
      assert.equal(new URL(page.url()).searchParams.get('ui'), options.mobile ? 'mobile' : 'desktop');
      if (!options.mobile) {
        assert.equal(await page.evaluate(() => window.previousWorkspace.isConnected), true);
        assert.equal(await page.evaluate(() => history.length), depth);
      }
      await page.locator('.agent-task-notice').waitFor({ state: 'detached' });
      // A later notice must also reveal an already-selected minimized tab.
      if (!options.mobile && (options.floating || options.global)) {
        await page.getByRole('button', { name: '最小化浮动工作区', exact: true }).press('Enter');
        await active.filter({ hasText: 'Target Tab' }).waitFor({ state: 'hidden' });
        await (await notifyTab(page, 'target', root)).click();
        await active.filter({ hasText: 'Target Tab' }).waitFor();
        await page.locator('.agent-task-notice').waitFor({ state: 'detached' });
      }
      assert.equal(await active.filter({ hasText: 'Target Tab' }).count(), 1);
    });
  });

  await test('activating a tab clears its notices on desktop and mobile while preserving unrelated notices', async t => {
    for (const mobile of [false, true]) await t.test(mobile ? 'mobile' : 'desktop', async t => {
      const { page, root } = await fixture(t, { mobile, entry: 'other' });
      await expectPath(page, 'other');
      await notifyTab(page, 'target', root);
      await notifyTab(page, 'target', root); // Multiple turns in the same session.
      await notifyTab(page, 'main-tab', worktrees[0].path);
      await notifyTab(page, 'target', root, { session_id: 'legacy', sources: undefined });
      // Matching any source acknowledges the entire notice.
      await notifyTab(page, 'target', root, { session_id: 'multi-source', sources: [
        { tab_id: 'main-tab', tab_name: 'main-tab', project_name: 'Link Fixture', workspace_root: worktrees[0].path },
        { tab_id: 'target', tab_name: 'target', project_name: 'Link Fixture', workspace_root: root },
      ] });
      await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 5);
      if (!mobile) {
        await expectUnreadCount(page, root, 3);
        await expectUnreadCount(page, worktrees[0].path, 2);
      }
      // An already-active tab's stop must not evict an unread notice from the full queue.
      await notifyTab(page, 'other', root);
      const target = mobile ? page.getByRole('tab', { name: 'Target Tab', exact: true }) : desktopTab(page, 'Target Tab');
      // Use the keyboard on narrow screens where the toast stack covers the tab strip.
      if (mobile) await target.press('Enter');
      else await target.click();
      await expectPath(page, 'target');
      await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 2);
      if (!mobile) {
        await expectUnreadCount(page, root, 0);
        await expectUnreadCount(page, worktrees[0].path, 1);
      }
      assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(), ['Tab 不可用', 'main-tab']);
      assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · main-tab', exact: true }).count(), 1);
      assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · target', exact: true }).count(), 0);
      const other = mobile ? page.getByRole('tab', { name: 'Other Tab', exact: true }) : desktopTab(page, 'Other Tab');
      if (mobile) await other.press('Enter');
      else await other.click();
      await expectPath(page, 'other');
      await (await notifyTab(page, 'target', root)).waitFor();
      assert.equal(await page.locator('.agent-task-notice').count(), 3);
    });
  });

  await test('folded notices clear on tab activation and the expanded queue stays open during navigation', async t => {
    const { page, root } = await fixture(t, { entry: 'other' });
    await expectPath(page, 'other');
    for (let index = 0; index < 3; index++) await notifyTab(page, 'target', root);
    for (let index = 0; index < 6; index++) await notifyTab(page, `closed-${index}`, root);
    await page.getByRole('button', { name: /还有 4 条通知/ }).waitFor();
    await desktopTab(page, 'Target Tab').click();
    await expectPath(page, 'target');
    await page.getByRole('button', { name: /还有 1 条通知/ }).waitFor();
    await desktopTab(page, 'Other Tab').click();
    await expectPath(page, 'other');
    await page.reload();
    await expectPath(page, 'other');
    await page.getByRole('button', { name: /还有 1 条通知/ }).click();
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 6);
    await expectPath(page, 'other');
    assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · target', exact: true }).count(), 0);
    await (await notifyTab(page, 'target', root)).click();
    await expectPath(page, 'target');
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 6);
    await page.getByRole('button', { name: '收起', exact: true }).waitFor();
    await page.reload();
    await expectPath(page, 'target');
    await page.getByRole('button', { name: /还有 1 条通知/ }).click();
    await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 6);
    assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · target', exact: true }).count(), 0);
  });

  await test('workspace activation and floating tab focus dismiss their associated notices', async t => {
    const { page, root } = await fixture(t, { floating: true });
    const floatingTab = page.locator('[data-floating-workspace] .project-aow-center-tab').filter({ hasText: 'Target Tab' });
    await floatingTab.waitFor();
    await expectPath(page, 'target');
    await desktopTab(page, 'Other Tab').click();
    await expectPath(page, 'other');
    await (await notifyTab(page, 'target', root)).waitFor();
    await floatingTab.click();
    await expectPath(page, 'target');
    await page.locator('.agent-task-notice').waitFor({ state: 'detached' });
    await (await notifyTab(page, 'main-tab', worktrees[0].path)).waitFor();
    await page.locator('.project-aow-worktrees button[title="/workspace/main"]').click();
    await expectPath(page, 'main-tab');
    await page.locator('.agent-task-notice').waitFor({ state: 'detached' });
  });

  await test('notification cards support Enter and Space without opening another browser tab', async t => {
    for (const key of ['Enter', 'Space']) await t.test(key, async t => {
      const { page } = await fixture(t, { entry: '' });
      await expectPath(page, 'main-tab');
      await notifyTab(page, 'other');
      const card = await notifyTab(page);
      await card.press(key);
      await desktopTarget(page).waitFor();
      await page.waitForFunction(() => document.querySelectorAll('.agent-task-notice').length === 1);
      assert.equal(await page.getByRole('button', { name: '打开通知：Link Fixture · other', exact: true }).count(), 1);
      await expectPath(page, 'target');
      assert.equal(page.context().pages().length, 1);
    });
  });

  await test('an expired notification can be removed permanently without replacing the current workspace', async t => {
    const { page } = await fixture(t, { entry: '' });
    await expectPath(page, 'main-tab');
    const currentUrl = page.url();
    const missing = await notifyTab(page, 'deleted-tab');
    await missing.click();
    await page.locator('.agent-task-notice [role="alert"]').filter({ hasText: '该 Tab 已关闭或不存在。' }).waitFor();
    await desktopTab(page, 'Main Tab').waitFor();
    assert.equal(page.url(), currentUrl);
    assert.equal(page.context().pages().length, 1);
    await page.getByRole('button', { name: '移除通知', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await page.reload();
    await expectPath(page, 'main-tab');
    const target = await notifyTab(page);
    await target.click();
    await desktopTarget(page).waitFor();
    await expectPath(page, 'target');
    assert.equal(await page.locator('.agent-task-notice [role="alert"]').count(), 0);
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
  });

  await test('a notification with several sources opens the first available tab', async t => {
    const { page, root } = await fixture(t, { entry: '' });
    await expectPath(page, 'main-tab');
    await notifyTab(page, 'target', root, { sources: ['deleted-tab', 'target'].map(tab_id => ({
      tab_id, tab_name: tab_id, project_name: 'Link Fixture', workspace_root: root,
    })) });
    await page.getByRole('button', { name: '打开通知：Link Fixture · deleted-tab、target', exact: true }).click();
    await desktopTarget(page).waitFor();
    await expectPath(page, 'target');
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
  });

  await test('desktop restores, switches worktrees, creates and closes tabs without reloading', async t => {
    const { page, state } = await fixture(t, { entry: '', editable: true });
    await expectPath(page, 'main-tab');
    await page.evaluate(() => { window.routeTestMarker = true; });
    const depth = await page.evaluate(() => history.length);
    await page.locator('.project-aow-worktrees button[title="/workspace/linked"]').click();
    await expectPath(page, 'other');
    await desktopTab(page, 'Target Tab').click();
    await expectPath(page, 'target');
    assert.equal(await page.evaluate(() => history.length), depth);
    assert.equal(await page.evaluate(() => window.routeTestMarker), true);
    await page.reload();
    await desktopTarget(page).waitFor();
    await page.getByRole('button', { name: '新建窗体', exact: true }).click();
    await page.getByRole('button', { name: "终端", exact: true }).click();
    await expectPath(page, 'created');
    for (const [name, next] of [['Created Tab', 'target'], ['Target Tab', 'other'], ['Other Tab', undefined]]) {
      await page.getByRole('button', { name: `关闭 ${name}`, exact: true }).click();
      await expectPath(page, next);
    }
    assert.deepEqual(state.mutations, ['create']);
  });

  await test('floating and project focus each publish their own active tab', async t => {
    const { page, state } = await fixture(t, { floating: true, editable: true });
    const floatingTab = page.locator('[data-floating-workspace] .project-aow-center-tab').filter({ hasText: 'Target Tab' });
    await floatingTab.waitFor();
    await expectPath(page, 'target');
    await desktopTab(page, 'Other Tab').click();
    await expectPath(page, 'other');
    await floatingTab.click();
    await expectPath(page, 'target');
    await floatingTab.getByRole('button', { name: '关闭 Target Tab', exact: true }).click();
    await expectPath(page, 'other');
    assert.deepEqual(state.mutations, []);
  });

  await test('mobile updates tab URLs and preserves home, back and forward navigation', async t => {
    const { page } = await fixture(t, { mobile: true, entry: '' });
    await page.getByRole('button', { name: /dev linked/ }).click();
    await expectPath(page, 'other');
    await page.getByRole('tab', { name: 'Target Tab', exact: true }).click();
    await expectPath(page, 'target');
    await page.goBack();
    await page.getByRole('heading', { name: '你的项目' }).waitFor();
    await expectPath(page);
    await page.goForward();
    await page.locator('.mobile-terminal-tabs [aria-selected="true"]').filter({ hasText: 'Target Tab' }).waitFor();
    await expectPath(page, 'target');
    await page.getByRole('tab', { name: 'Other Tab', exact: true }).click();
    await expectPath(page, 'other');
    await page.getByRole('button', { name: '文件', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/files');
    await page.reload();
    await page.locator('.mobile-workspace[data-view="files"]').waitFor();
    await page.getByRole('button', { name: '终端', exact: true }).click();
    await expectPath(page, 'other');
  });
  const resourceUrl = (type, params) => '/aow/tabs/' + type + '?' + new URLSearchParams({ workspace: 'wt-1', ...params });
  const activeDesktop = page => page.locator('.project-aow-center-tab.active:visible');
  async function restoreAndSwitch(page, mobile, path, label, ready) {
    await ready();
    await page.waitForURL(url => url.pathname === path);
    const before = new URL(page.url());
    if (!mobile) {
      await desktopTab(page, 'Other Tab').click();
      await expectPath(page, 'other');
      await page.locator('.project-aow-center-tab:visible').filter({ hasText: label }).click();
      await page.waitForURL(url => url.pathname === before.pathname && url.search === before.search);
    }
    await page.reload();
    await ready();
    await page.waitForURL(url => url.pathname === before.pathname && url.search === before.search);
  }

  await test('file tabs restore project, notes and external paths in a fresh browser and after refresh', async t => {
    for (const mobile of [false, true]) for (const source of ['project', 'notes', 'external']) {
      const path = ({ project: '/workspace/linked', notes: '/notes', external: '/tmp' })[source] + '/read me #中文.md';
      const { page, state } = await fixture(t, { mobile, tabUrl: resourceUrl('file', { path, source }) });
      const ready = () => mobile ? page.getByRole('heading', { name: 'Linked document', exact: true }).waitFor()
        : activeDesktop(page).filter({ hasText: 'read me #中文.md' }).waitFor();
      await restoreAndSwitch(page, mobile, '/aow/tabs/file', 'read me #中文.md', ready);
      assert.equal(new URL(page.url()).searchParams.get('path'), path);
      assert.equal(new URL(page.url()).searchParams.get('source'), source);
      await page.waitForFunction(() => document.querySelector('.monaco-editor') || document.querySelector('.mobile-markdown'));
      assert.ok(state.reads.some(url => decodeURIComponent(url).includes('/api/fs/text' + path)));
    }
  });

  for (const mobile of [false, true]) await test(`Markdown file links resolve from the source directory on ${mobile ? 'mobile' : 'desktop'}`, async t => {
    const directory = '/workspace/linked/reports #中文%';
    const path = directory + '/result.md';
    const textPath = directory + '/evidence/api/01-preflight-failure.txt';
    const markdown = [
      '| Case | Result | Image | ANSI | Text |',
      '| --- | --- | --- | --- | --- |',
      '| api / 01-preflight-failure | 预检失败（夹具已修正） | [查看](evidence/api/01-preflight-failure.png) | [ANSI](evidence/api/01-preflight-failure.ansi.txt) | [文本](evidence/api/01-preflight-failure.txt) |',
      '',
      '[Absolute](/tmp/evidence/result.txt)',
      '[Current](./evidence/./result.txt)',
      '[Parent](../shared/../result.txt)',
      '[Encoded](evidence/read%20me%20%23中文%3F%25.txt)',
      '[Fragment](other.md#section)',
      '[Anchor](#section)',
      '[HTTPS](https://example.com/evidence/result.txt)',
      '[HTTP](http://example.com/result.txt)',
      'http://example.com/plain-http',
      'https://example.com/plain-https',
      '[Protocol relative](//example.com/result.txt)',
      '[Email](mailto:reader@example.com)',
      '<a href="evidence/raw.txt">HTML link</a>',
      '<a href="https://example.com/html">HTML HTTPS</a>',
      '[Unsafe](javascript:alert%281%29)',
      '<a href="javascript:alert(1)">Unsafe HTML</a>',
    ].join('\n');
    const { page, state } = await fixture(t, { mobile, tabUrl: resourceUrl('file', { path }), fileContents: { [path]: markdown, [textPath]: 'Evidence text opened through the file route.' } });
    if (!mobile) await page.getByRole('button', { name: '切换到预览', exact: true }).click();
    const preview = page.locator(mobile ? '.mobile-markdown' : '.markdown-preview');
    await preview.getByRole('link', { name: '文本', exact: true }).waitFor();
    for (const [name, expectedPath, hash = ''] of [
      ['查看', directory + '/evidence/api/01-preflight-failure.png'],
      ['ANSI', directory + '/evidence/api/01-preflight-failure.ansi.txt'],
      ['文本', textPath],
      ['Absolute', '/tmp/evidence/result.txt'],
      ['Current', directory + '/evidence/result.txt'],
      ['Parent', '/workspace/linked/result.txt'],
      ['Encoded', directory + '/evidence/read me #中文?%.txt'],
      ['Fragment', directory + '/other.md', '#section'],
      ['HTML link', directory + '/evidence/raw.txt'],
    ]) {
      const link = preview.getByRole('link', { name, exact: true });
      const href = await link.getAttribute('href');
      assert.equal(await link.getAttribute('target'), null);
      assert.ok(href.startsWith('/aow/tabs/file?'), href);
      const url = new URL(href, base);
      assert.equal(url.searchParams.get('workspace'), 'wt-1');
      assert.equal(url.searchParams.get('path'), expectedPath);
      assert.equal(url.hash, hash);
    }
    for (const [name, href] of [['Anchor', '#section'], ['HTTPS', 'https://example.com/evidence/result.txt'],
      ['HTTP', 'http://example.com/result.txt'], ['Protocol relative', '//example.com/result.txt'], ['Email', 'mailto:reader@example.com']]) {
      assert.equal(await preview.getByRole('link', { name, exact: true }).getAttribute('href'), href);
    }
    await page.context().route(/^https?:\/\/example\.com\//, route => route.fulfill({ contentType: 'text/html', body: '<p>External destination</p>' }));
    const sourceUrl = page.url();
    for (const name of ['HTTP', 'HTTPS', 'HTML HTTPS', 'http://example.com/plain-http', 'https://example.com/plain-https']) {
      const link = preview.getByRole('link', { name, exact: true });
      const href = await link.getAttribute('href');
      assert.equal(await link.getAttribute('target'), '_blank');
      assert.equal(await link.getAttribute('rel'), 'noreferrer');
      const popupPromise = page.waitForEvent('popup');
      await link.click();
      const popup = await popupPromise;
      await popup.waitForLoadState();
      assert.equal(popup.url(), href);
      assert.equal(await popup.evaluate(() => window.opener), null);
      assert.equal(page.url(), sourceUrl);
      await popup.close();
    }
    for (const name of ['Unsafe', 'Unsafe HTML']) assert.equal(await preview.getByText(name, { exact: true }).getAttribute('href'), null);
    const pageCount = page.context().pages().length;
    await page.evaluate(() => { window.markdownFileNavigationMarker = true; });
    await preview.getByRole('link', { name: '文本', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/file' && url.searchParams.get('path') === textPath);
    await page.getByText('Evidence text opened through the file route.', { exact: true }).first().waitFor();
    assert.equal(new URL(page.url()).searchParams.get('workspace'), 'wt-1');
    assert.equal(page.context().pages().length, pageCount);
    assert.equal(await page.evaluate(() => window.markdownFileNavigationMarker), true, 'local files open without reloading the page');
    if (!mobile) {
      await desktopTab(page, 'result.md').waitFor();
      await activeDesktop(page).filter({ hasText: '01-preflight-failure.txt' }).waitFor();
    }
    assert.ok(state.reads.some(url => decodeURIComponent(url).includes('/api/fs/text' + textPath)));
  });

  for (const surface of ['ACP', 'Conversation']) for (const mobile of [false, true]) await test(`${surface} Markdown links share file routing on ${mobile ? 'mobile' : 'desktop'}`, async t => {
    const directory = surface === 'ACP' ? '/workspace/linked' : '/actual-session';
    const textPath = directory + '/evidence/result.txt';
    const imagePath = directory + '/evidence/preview #中文.png';
    const markdown = [
      '| Text | Image |\n| --- | --- |\n| [Local text](evidence/result.txt) | [Local image](evidence/preview%20%23中文.png) |',
      '', '[Absolute](/tmp/result.txt)', '[Parent](../shared/result.txt)',
      '[File URI](file:///tmp/read%20me.txt#L12)', '[Line](source.ts:8)',
      '<a href="evidence/raw.txt">HTML local</a>',
      '[HTTPS](https://example.com/result.txt)', 'http://example.com/plain',
      '[Remote image](https://example.com/remote.png)', '[Unsafe](javascript:alert%281%29)',
    ].join('\n\n');
    const acp = acpSession();
    acp.entries[1].content.text = markdown;
    const { page, state } = await fixture(t, { mobile, acp: surface === 'ACP', acpHistory: [acp], sessionMarkdown: markdown,
      tabUrl: surface === 'ACP' ? '/aow/tabs/acp/acp-local' : resourceUrl('session/codex/session-one', { cwd: directory }),
      fileContents: { [textPath]: 'Evidence from the conversation.' } });
    const body = page.locator(surface === 'ACP' ? '.acp-markdown:visible' : mobile ? '.mobile-message.assistant' : '.project-aow-snapshot-conclusion');
    await body.getByRole('link', { name: 'Local text', exact: true }).waitFor();
    for (const [name, expected, hash = ''] of [
      ['Local text', textPath], ['Local image', imagePath], ['Absolute', '/tmp/result.txt'],
      ['Parent', surface === 'ACP' ? '/workspace/shared/result.txt' : '/shared/result.txt'],
      ['File URI', '/tmp/read me.txt', '#L12'], ['Line', directory + '/source.ts', '#L8'], ['HTML local', directory + '/evidence/raw.txt'],
    ]) {
      const link = body.getByRole('link', { name, exact: true });
      const href = await link.getAttribute('href');
      assert.ok(href.startsWith('/aow/tabs/file?'), href);
      const url = new URL(href, base);
      assert.equal(url.searchParams.get('workspace'), 'wt-1');
      assert.equal(url.searchParams.get('path'), expected);
      assert.equal(url.hash, hash);
      assert.equal(await link.getAttribute('target'), null);
    }
    assert.equal(await body.getByText('Unsafe', { exact: true }).getAttribute('href'), null);
    const imageRequests = [];
    const externalRequests = [];
    await page.context().route('**/api/fs/raw/**', async route => {
      imageRequests.push(decodeURIComponent(new URL(route.request().url()).pathname));
      await route.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"><rect width="80" height="60" fill="green"/></svg>' });
    });
    await page.context().route(/^https?:\/\/example\.com\//, async route => {
      externalRequests.push(route.request().url());
      await route.fulfill({ contentType: 'text/html', body: '<p>External destination</p>' });
    });
    if (!mobile) {
      assert.deepEqual(imageRequests, []);
      await body.getByRole('link', { name: 'Local image', exact: true }).hover();
      const tooltip = page.locator('.markdown-image-tooltip');
      await tooltip.getByRole('img').waitFor();
      assert.deepEqual(imageRequests, ['/api/fs/raw' + imagePath]);
      await page.keyboard.press('Escape');
      await tooltip.waitFor({ state: 'hidden' });
      await body.getByRole('link', { name: 'Remote image', exact: true }).hover();
      await page.waitForTimeout(250);
      assert.equal(await tooltip.count(), 0);
      assert.deepEqual(externalRequests, []);
    }
    const sourceUrl = page.url();
    for (const name of ['HTTPS', 'http://example.com/plain']) {
      const link = body.getByRole('link', { name, exact: true });
      assert.equal(await link.getAttribute('target'), '_blank');
      assert.equal(await link.getAttribute('rel'), 'noreferrer');
      const popupPromise = page.waitForEvent('popup');
      await link.click();
      const popup = await popupPromise;
      await popup.waitForLoadState();
      assert.equal(popup.url(), await link.getAttribute('href'));
      assert.equal(page.url(), sourceUrl);
      await popup.close();
    }
    const pageCount = page.context().pages().length;
    await page.evaluate(() => { window.conversationLinkMarker = true; });
    await body.getByRole('link', { name: 'Local text', exact: true }).click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/file' && url.searchParams.get('path') === textPath);
    await page.getByText('Evidence from the conversation.', { exact: true }).first().waitFor();
    assert.equal(new URL(page.url()).searchParams.get('workspace'), 'wt-1');
    assert.equal(page.context().pages().length, pageCount);
    assert.equal(await page.evaluate(() => window.conversationLinkMarker), true);
    if (!mobile) await desktopTab(page, surface === 'ACP' ? 'Saved ACP acp-local' : 'Linked Conversation').waitFor();
    assert.deepEqual(state.errors, []);
  });

  await test('Markdown local image links preview on hover without fetching external images', async t => {
    const path = '/workspace/linked/reports #中文%/images.md';
    const imagePath = '/workspace/linked/reports #中文%/evidence/preview #中文.png';
    const absolutePath = '/tmp/absolute image.JPG';
    const markdown = '[Relative image](evidence/preview%20%23中文.png)\n\n[Absolute image](</tmp/absolute image.JPG>)\n\n[Missing image](missing.png)\n\n[Text](evidence/result.txt)\n\n[HTTPS image](https://example.com/remote.png)\n\n[HTTP image](http://example.com/remote.png)\n\n[Protocol image](//example.com/remote.png)';
    const { page } = await fixture(t, { tabUrl: resourceUrl('file', { path }), fileContents: { [path]: markdown } });
    const png = await page.evaluate(() => {
      const canvas = document.createElement('canvas'); canvas.width = 1200; canvas.height = 1800;
      const context = canvas.getContext('2d');
      context.fillStyle = '#243b59'; context.fillRect(0, 0, canvas.width, canvas.height);
      context.fillStyle = '#d3def0'; context.font = '64px sans-serif'; context.fillText('Local image preview', 48, 120);
      context.fillStyle = '#4ec9b0'; context.fillRect(48, 1520, 1104, 220);
      return canvas.toDataURL('image/png').split(',')[1];
    });
    const imageRequests = [];
    const externalRequests = [];
    let releaseImage;
    const imageReady = new Promise(resolve => { releaseImage = resolve; });
    t.after(() => releaseImage());
    await page.context().route('**/api/fs/raw/**', async route => {
      const requested = decodeURIComponent(new URL(route.request().url()).pathname.slice('/api/fs/raw'.length));
      imageRequests.push(requested);
      if (requested.endsWith('/missing.png')) { await route.fulfill({ status: 404 }); return; }
      await imageReady;
      await route.fulfill({ contentType: 'image/png', body: Buffer.from(png, 'base64') });
    });
    await page.context().route(/^https?:\/\/example\.com\//, async route => { externalRequests.push(route.request().url()); await route.abort(); });
    await page.getByRole('button', { name: '切换到预览', exact: true }).click();
    const preview = page.locator('.markdown-preview');
    const relative = preview.getByRole('link', { name: 'Relative image', exact: true });
    const absolute = preview.getByRole('link', { name: 'Absolute image', exact: true });
    const tooltip = page.getByRole('tooltip');
    await relative.waitFor();
    assert.deepEqual(imageRequests, [], 'images load only when requested by hovering');
    await relative.hover();
    await tooltip.getByText('正在加载图片…', { exact: true }).waitFor();
    assert.deepEqual(imageRequests, [imagePath]);
    releaseImage();
    await tooltip.getByRole('img').waitFor();
    assert.ok(await tooltip.getByRole('img').evaluate(img => img.complete && img.naturalWidth > 0));
    const imageBounds = await tooltip.getByRole('img').boundingBox();
    assert.ok(imageBounds.height <= 240 && imageBounds.width <= 322, 'large images fit inside the preview');
    assert.ok(Math.abs(imageBounds.width / imageBounds.height - 2 / 3) < 0.01, 'the preview preserves the image aspect ratio');
    await page.screenshot({ path: join(screenshotDir, 'markdown-image-hover.png') });
    assert.equal(decodeURIComponent(new URL(await tooltip.getByRole('img').getAttribute('src'), base).pathname), '/api/fs/raw' + imagePath);
    assert.equal(await relative.getAttribute('aria-describedby'), await tooltip.getAttribute('id'));
    await tooltip.hover();
    await page.waitForTimeout(250);
    assert.equal(await tooltip.isVisible(), true, 'the pointer can move into the preview');
    await page.keyboard.press('Escape');
    await tooltip.waitFor({ state: 'hidden' });
    assert.equal(await relative.getAttribute('aria-describedby'), null);
    for (const name of ['Text', 'HTTPS image', 'HTTP image', 'Protocol image']) {
      await preview.getByRole('link', { name, exact: true }).hover();
      await page.waitForTimeout(250);
      assert.equal(await tooltip.count(), 0);
    }
    assert.deepEqual(externalRequests, []);
    assert.deepEqual(imageRequests, [imagePath]);
    await absolute.evaluate(element => { Object.assign(element.style, { position: 'fixed', right: '2px', bottom: '2px' }); });
    await absolute.hover();
    await tooltip.getByRole('img').waitFor();
    assert.equal(decodeURIComponent(new URL(await tooltip.getByRole('img').getAttribute('src'), base).pathname), '/api/fs/raw' + absolutePath);
    const bounds = await tooltip.boundingBox();
    assert.ok(bounds.x >= 8 && bounds.y >= 8 && bounds.x + bounds.width <= 1432 && bounds.y + bounds.height <= 892);
    await absolute.evaluate(element => element.removeAttribute('style'));
    await preview.evaluate(element => element.dispatchEvent(new Event('scroll')));
    await tooltip.waitFor({ state: 'hidden' });
    await relative.focus();
    await page.keyboard.press('Tab');
    await page.getByRole('tooltip', { name: '图片预览：absolute image.JPG' }).waitFor();
    await page.keyboard.press('Escape');
    await preview.getByRole('link', { name: 'Missing image', exact: true }).hover();
    await tooltip.getByText('图片预览加载失败', { exact: true }).waitFor();
    await relative.hover();
    await tooltip.getByRole('img').waitFor();
    await page.evaluate(() => { window.markdownImageNavigationMarker = true; });
    await relative.click();
    await page.waitForURL(url => url.pathname === '/aow/tabs/file' && url.searchParams.get('path') === imagePath);
    await page.locator('.image-preview img').waitFor();
    await tooltip.waitFor({ state: 'hidden' });
    assert.equal(await page.evaluate(() => window.markdownImageNavigationMarker), true);
    await desktopTab(page, 'images.md').waitFor();
  });

  await test('file browser restores its directory and publishes opened file URLs', async t => {
    for (const mobile of [false, true]) {
      const { page } = await fixture(t, { mobile, tabUrl: resourceUrl('files', { path: '/tmp' }) });
      if (mobile) await page.locator('.mobile-list-row').filter({ hasText: 'read me #中文.md' }).click();
      else {
        await page.locator('.system-file-footer').filter({ hasText: '/tmp' }).waitFor();
        await page.reload();
        await page.locator('.system-file-footer').filter({ hasText: '/tmp' }).waitFor();
        await page.getByRole('button', { name: '打开 read me #中文.md', exact: true }).click();
      }
      await page.waitForURL(url => url.pathname === '/aow/tabs/file');
      assert.equal(new URL(page.url()).searchParams.get('path'), '/tmp/read me #中文.md');
      assert.equal(new URL(page.url()).searchParams.get('source'), 'external');
    }
  });

  await test('working, staged and commit diffs retain repository, source and rename metadata', async t => {
    for (const mobile of [false, true]) for (const source of ['working', 'staged', 'commit']) {
      const commit = 'a'.repeat(40);
      const params = { repository: '/repo/nested', path: 'renamed #中文.ts', source,
        ...(source === 'commit' ? { commit, original_path: 'old name.ts' } : {}) };
      const { page, state } = await fixture(t, { mobile, tabUrl: resourceUrl('diff', params) });
      const ready = () => mobile ? page.locator('.mobile-diff').filter({ hasText: 'after' }).waitFor()
        : activeDesktop(page).filter({ hasText: 'renamed #中文.ts' }).waitFor();
      await restoreAndSwitch(page, mobile, '/aow/tabs/diff', 'renamed #中文.ts', ready);
      assert.equal(new URL(page.url()).searchParams.get('source'), source);
      const request = state.reads.map(path => new URL(path, base)).find(url => url.pathname === (source === 'commit' ? '/api/git/commit/diff' : '/api/git/diff'));
      assert.equal(request.searchParams.get('repo'), params.repository);
      if (source === 'commit') { assert.equal(request.searchParams.get('commit'), commit); assert.equal(request.searchParams.get('original_path'), 'old name.ts'); }
      else assert.equal(request.searchParams.get('staged'), String(source === 'staged'));
    }
  });

  await test('PR and conversation routes reopen their exact resource on desktop and mobile', async t => {
    for (const mobile of [false, true]) {
      const pr = await fixture(t, { mobile, tabUrl: resourceUrl('pr/custom/42', { repository: '/repo/pr', remote: 'upstream' }) });
      await restoreAndSwitch(pr.page, mobile, '/aow/tabs/pr/custom/42', 'PR #42', () => pr.page.getByRole('heading', { name: 'Linked PR', exact: true }).waitFor());
      assert.equal(new URL(pr.page.url()).searchParams.get('remote'), 'upstream');
      assert.ok(pr.state.reads.some(path => new URL(path, base).searchParams.get('repo') === '/repo/pr'));
      const session = await fixture(t, { mobile, tabUrl: resourceUrl('session/codex/session-one', { cwd: '/actual-session' }) });
      await restoreAndSwitch(session.page, mobile, '/aow/tabs/session/codex/session-one', 'Linked Conversation', () => session.page.getByText('Restored conclusion', { exact: true }).waitFor());
      const request = session.state.reads.find(path => path.includes('/session-one/snapshot'));
      assert.equal(new URL(request, base).searchParams.get('worktree_path'), '/actual-session');
    }
    const ordinary = await fixture(t, { sessionAgent: true });
    await desktopTarget(ordinary.page).waitFor();
    await ordinary.page.getByRole('button', { name: "对话", exact: true }).click();
    await ordinary.page.locator('.project-aow-session-row').filter({ hasText: 'Linked Conversation' }).click();
    await ordinary.page.waitForURL(url => url.pathname === '/aow/tabs/session/codex/session-one' && url.searchParams.get('cwd') === '/actual-session');
  });

  await test('automation routes show tasks and load a selected run beyond the first history page', async t => {
    for (const mobile of [false, true]) for (const run of [false, true]) {
      const path = '/aow/tabs/automation/task-one' + (run ? '/runs/run-old' : '');
      const { page, state } = await fixture(t, { mobile, tabUrl: path + '?workspace=wt-1' });
      const ready = () => run ? page.getByText('run-old', { exact: true }).first().waitFor()
        : page.getByRole('heading', { name: 'Linked Automation', exact: true }).waitFor();
      await restoreAndSwitch(page, mobile, path, 'Linked Automation', ready);
      if (run) assert.ok(state.reads.some(path => path.includes('/task-one/runs/run-old')));
    }
  });

  await test('mobile PR list selects a remote and carries provider into its detail route', async t => {
    const { page, state } = await fixture(t, { mobile: true,
      tabUrl: '/aow/#workspace=%2Fworkspace%2Flinked&view=pull-requests',
      reviewTargets: [
        { remote: 'origin', host: 'github.com', repository: 'team/project', provider: 'github', provider_name: 'GitHub' },
        { remote: 'upstream', host: 'git.example.com', repository: 'team/project', provider: 'custom', provider_name: 'Custom' },
      ],
    });
    const picker = page.getByLabel("PR 远程仓库", { exact: true });
    await picker.waitFor();
    assert.equal(state.reads.filter(path => path.startsWith('/api/my-pull-requests')).length, 0);
    await picker.selectOption('upstream');
    await page.locator('.mobile-pr-card').click();
    await page.getByRole('heading', { name: 'Linked PR', exact: true }).waitFor();
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/pr/custom/42');
    assert.equal(new URL(page.url()).searchParams.get('remote'), 'upstream');
    assert.ok(state.reads.some(path => path.startsWith('/api/my-pull-requests/42') && new URL(path, base).searchParams.get('provider') === 'custom'));
  });

  await test('invalid typed routes explain the problem without creating resources', async t => {
    for (const [url, error] of [
      [resourceUrl('file', { path: '/tmp/file', source: 'invalid' }), '无法识别文件来源。'],
      [resourceUrl('diff', { repository: '/repo', path: 'file', source: 'commit', commit: 'main' }), 'Commit Diff 必须使用完整提交 SHA。'],
      [resourceUrl('pr/missing/42', { repository: '/repo' }), 'Provider 未配置或已停用。'],
    ]) {
      const { page } = await fixture(t, { tabUrl: url });
      await page.getByRole('alert').filter({ hasText: error }).waitFor();
    }
  });

} finally {
  await browser?.close();
  await server.close();
}
