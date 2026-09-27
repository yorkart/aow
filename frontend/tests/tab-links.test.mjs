import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

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
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  async function fixture(t, { mobile = false, login = false, floating = false, extraFloatingTabs = [], global = false, missing = false, removedWorkspace = false, entry = 'target', editable = false, tabUrl, sessionAgent = false, pinned = [], reviewTargets = [] } = {}) {
    const context = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1440, height: 900 } });
    const root = global ? '/global' : worktrees[1].path;
    const target = terminal('target', root, 'Target Tab');
    const other = terminal('other', root, 'Other Tab');
    const tabs = [terminal('main-tab', worktrees[0].path, 'Main Tab'), other, target];
    const projects = removedWorkspace ? [] : [project];
    if (global) projects.push({ ...project, id: '__aow_floating', name: '浮动工作区', builtin: true,
      registered_path: root, worktrees: [{ ...worktrees[0], id: 'global-main', project_id: '__aow_floating', path: root }] });
    const state = { authenticated: !login, targetReads: 0, mutations: [], errors: [], reads: [] };
    const session = { id: 'codex:session-one', session_id: 'session-one', agent: 'codex', title: 'Linked Conversation', cwd: '/actual-session', created_at: '', updated_at: '' };
    const task = { id: 'task-one', name: 'Linked Automation', project_id: 'project', project_name: project.name, revision: 1, agent: 'codex', workspace_mode: 'existing', workspace_path: root, prompt: 'Task prompt', cron: '0 9 * * *', interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: false, base_branch: '', cleanup_worktree: false, precheck_command: '', precheck_timeout_seconds: 30, is_running: false, scheduler_error: null, last_run: null, next_run_at: null };
    const run = { id: 'run-old', task_id: task.id, task_name: task.name, agent: 'codex', source: 'manual', status: 'completed', started_at: '2026-09-19T00:00:00Z', finished_at: '2026-09-19T00:01:00Z', workspace_path: root, branch: 'dev', session_id: null, duration_ms: 60000, exit_code: 0 };
    const pr = { number: 42, title: 'Linked PR', source_branch: 'dev', target_branch: 'main', status: 'open', draft: false, created_at: '', updated_at: '', url: null, description: 'PR description', files: [], checks: [], reviewers: [], threads: [], unresolved_threads: [], changes_count: 0, commits_count: 1 };
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); if (!editable) assert.deepEqual(state.mutations, []); });
    await context.addInitScript(({ root, floating, extraFloatingTabs }) => {
      const taskStopSources = new Set();
      window.EventSource = class extends EventSource {
        constructor(...args) { super(...args); taskStopSources.add(this); }
        close() { taskStopSources.delete(this); super.close(); }
      };
      window.sendTaskStop = data => {
        for (const source of taskStopSources) source.dispatchEvent(new MessageEvent('task-stopped', { data: JSON.stringify(data) }));
      };
      window.taskStopReady = () => taskStopSources.size > 0;
      localStorage.setItem('aow-active', '/workspace/main');
      localStorage.setItem(`aow-workspace-tabs:${root}`, JSON.stringify({ active: 'terminal:other' }));
      sessionStorage.setItem(`aow.mobile.terminal.${root}`, 'other:other-pane');
      if (floating) {
        localStorage.setItem('aow-floating-tabs-v1', JSON.stringify([{ workspace: root,
          id: 'terminal:target', targetId: 'target', kind: 'terminal', label: 'Target Tab' }, ...extraFloatingTabs]));
        localStorage.setItem('aow-floating-open', 'false');
      }
    }, { root, floating, extraFloatingTabs });
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      let data;
      if (request.method() === 'GET') state.reads.push(url.pathname + url.search);
      if (request.method() !== 'GET' && /\/api\/(fs|aow\/automations)\//.test(url.pathname)) state.mutations.push(request.method() + url.pathname);
      if (url.pathname === '/api/auth/status') data = { configured: true, authenticated: state.authenticated };
      else if (url.pathname === '/api/auth/login') { state.authenticated = true; data = { configured: true, authenticated: true }; }
      else if (!state.authenticated) { await route.fulfill({ status: 401, json: { message: 'Login required' } }); return; }
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
      else if (url.pathname === '/api/terminals' && request.method() === 'GET') data = tabs.filter(tab => tab.workspace_root === url.searchParams.get('workspace_root'));
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
      else if (url.pathname.startsWith('/api/fs/text')) data = { path: decodeURIComponent(url.pathname.slice('/api/fs/text'.length)), content: '# Linked document\n\nRestored from URL.', version: 'v1', language: 'markdown', mime: 'text/markdown', size: 38 };
      else if (url.pathname === '/api/git/diff' || url.pathname === '/api/git/commit/diff') data = { repository: url.searchParams.get('repo'), path: url.searchParams.get('path'), staged: url.searchParams.get('staged') === 'true', patch: '@@ -1 +1 @@\n-before\n+after\n', original: 'before\n', modified: 'after\n', truncated: false };
      else if (url.pathname === '/api/review-targets') data = reviewTargets;
      else if (url.pathname === '/api/my-pull-requests') data = { repository: root, current_branch: 'dev', current_user: { id: 'u', username: 'me' }, pull_requests: [{ ...pr, provider: url.searchParams.get('provider') || 'github', remote: url.searchParams.get('remote') || 'origin' }] };
      else if (url.pathname === '/api/my-pull-requests/42') {
        if (url.searchParams.get('provider') === 'missing') { await route.fulfill({ status: 503, json: { message: 'Provider 未配置或已停用。' } }); return; }
        data = { ...pr, provider: url.searchParams.get('provider') || 'github', remote: url.searchParams.get('remote') || 'origin' };
      }
      else if (url.pathname === '/api/aow/agent-sessions') data = url.searchParams.get('agent') === 'codex' ? [session] : [];
      else if (url.pathname.endsWith('/session-one/snapshot')) data = { ...session, status: 'completed', turns: [{ id: 'turn', status: 'completed', user: { text: 'Input', timestamp: null }, final: { text: 'Restored conclusion', timestamp: null } }], truncated: false, captured_at: '' };
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
    page.setDefaultTimeout(15000);
    page.on('pageerror', error => state.errors.push(`${page.url()}\n${error.stack ?? error.message}`));
    const destination = new URL(tabUrl ?? `/aow/${entry ? `tabs/${entry}` : ''}`, base);
    destination.searchParams.set('ui', mobile ? 'mobile' : 'desktop');
    await page.goto(destination.href);
    return { page, state, root };
  }

  const desktopTarget = page => page.locator('.project-aow-surface:not([hidden]) .project-aow-center-tab.active').filter({ hasText: 'Target Tab' });
  await test('deep link survives login and refresh and overrides the saved project and tab', async t => {
    const { page, state, root } = await fixture(t, { login: true });
    await page.getByLabel('PIN 码').waitFor();
    assert.equal(state.targetReads, 0);
    await page.getByLabel('PIN 码').fill('123456');
    await desktopTarget(page).waitFor();
    assert.equal(await page.locator('.project-aow-status').innerText().then(value => value.includes(root)), true);
    assert.match(page.url(), /\/aow\/tabs\/terminal\/target\?/);
    await page.reload();
    await desktopTarget(page).waitFor();
    await page.screenshot({ path: '/tmp/aow-tab-link-desktop.png' });
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
      assert.ok(badgeBox.x > rowBox.x + rowBox.width / 2);
      assert.ok(Math.abs(rowBox.x + rowBox.width - badgeBox.x - badgeBox.width - 8) <= 1);
      assert.ok(badgeBox.y >= rowBox.y && badgeBox.y + badgeBox.height <= rowBox.y + rowBox.height);
    }
    await page.screenshot({ path: '/tmp/aow-worktree-unread.png' });

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

  await test('closing popups preserves unread badges after reload until their source tabs activate', async t => {
    const { page, root } = await fixture(t, { entry: '', pinned: [worktrees[1].path] });
    await expectPath(page, 'main-tab');
    const expectCounts = async count => {
      for (const pinned of [false, true]) await expectUnreadCount(page, root, count, pinned);
    };
    await (await notifyTab(page, 'target', root)).waitFor();
    await (await notifyTab(page, 'other', root)).waitFor();
    await expectCounts(2);
    await page.getByRole('button', { name: '关闭通知：Link Fixture · target', exact: true }).click();
    await page.getByRole('button', { name: '打开通知：Link Fixture · target', exact: true }).waitFor({ state: 'detached' });
    await expectCounts(2);
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/terminal/main-tab');
    assert.equal(await page.locator('.agent-task-notice-tab').textContent(), 'other');

    await page.reload();
    await expectPath(page, 'main-tab');
    await expectCounts(2);
    assert.deepEqual(await page.locator('.agent-task-notice-tab').allTextContents(), ['other']);
    // A later turn in the same tab gets a new popup and a new unread entry.
    await (await notifyTab(page, 'target', root)).waitFor();
    await expectCounts(3);
    await page.getByText('2 条待处理', { exact: true }).waitFor();
    await page.screenshot({ path: '/tmp/aow-notification-close.png' });
    await page.getByRole('button', { name: '全部关闭', exact: true }).click();
    await page.locator('.agent-task-notifications').waitFor({ state: 'detached' });
    await expectCounts(3);
    await page.reload();
    await expectPath(page, 'main-tab');
    await expectCounts(3);
    assert.equal(await page.locator('.agent-task-notice').count(), 0);

    await page.locator(`.project-aow-pinned button[title="${root}"]`).click();
    await expectPath(page, 'other');
    await expectCounts(2);
    await desktopTab(page, 'Target Tab').click();
    await expectPath(page, 'target');
    await expectCounts(0);
    await page.reload();
    await expectPath(page, 'target');
    await expectCounts(0);
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
    await page.getByRole('button', { name: 'Terminal', exact: true }).click();
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
    await ordinary.page.getByRole('button', { name: 'Conversation', exact: true }).click();
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
    const picker = page.getByLabel('PR remote', { exact: true });
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
