import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, readFile } from 'node:fs/promises';
import { productionPreview } from './fixtures/production-preview.mjs';

const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const server = process.env.MOBILE_TEST_URL ? null : await productionPreview();
const baseURL = process.env.MOBILE_TEST_URL || `http://127.0.0.1:${server.httpServer.address().port}`;
const screenshots = process.env.MOBILE_TEST_SCREENSHOTS;
const workspace = '/workspace/aow';
const pane = (id, name, extra = {}) => ({ id, name, name_is_custom: false, cwd: workspace, shell: '/bin/bash', status: 'running', rows: 40, cols: 120, ...extra });
const leaf = (pane_id) => ({ type: 'pane', pane_id });
const tabs = [
  { id: 'tab-a', name: '开发', workspace_root: workspace, revision: 1,
    layout: { type: 'split', axis: 'row', ratio: .5, first: leaf('shell'), second: { type: 'split', axis: 'column', ratio: .4, first: leaf('codex'), second: leaf('logs') } },
    panes: [pane('logs', '日志'), pane('shell', 'Shell'), pane('codex', 'Codex', { kind: 'agent', agent_id: 'codex' })] },
  { id: 'tab-b', name: '服务', workspace_root: workspace, revision: 1, layout: leaf('server'), panes: [pane('server', 'Shell')] },
];
const project = { id: 'project', name: 'AOW', registered_path: workspace, common_git_dir: `${workspace}/.git`, notes_path: '/notes/aow',
  worktrees: [{ id: 'main', project_id: 'project', path: workspace, branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' },
    { id: 'feature', project_id: 'project', path: '/workspace/mobile', branch: 'feat/mobile-experience', head: 'abc', is_main: false, color: 'blue' }] };
const session = { id: 'session-one', agent: 'codex', session_id: 'session-one', title: '实现手机端工作台', cwd: workspace, created_at: '2026-09-11T08:00:00Z', updated_at: '2026-09-11T09:00:00Z' };
const automationRun = { id: 'run-one', task_id: 'task-one', task_revision: 1, task_name: '每日代码巡检', agent: 'codex', source: 'scheduled', status: 'completed',
  started_at: '2026-09-12T01:00:00Z', finished_at: '2026-09-12T01:02:03Z', workspace_path: workspace, branch: 'main', session_id: 'automation-session',
  agent_pid: 1234, agent_command: ['codex'], exit_code: 0, message: '执行完成', preparation_ms: 100, session_acquired_ms: 200, duration_ms: 123000 };
const automation = { id: 'task-one', revision: 1, name: '每日代码巡检', prompt: '# 巡检要求\n\n检查项目构建和测试结果。', agent: 'codex', project_id: 'project', project_name: 'AOW',
  workspace_mode: 'existing', workspace_path: workspace, cleanup_worktree: false, base_branch: '', cron: '0 9 * * *', interval_seconds: null, max_concurrent_runs: 1, enabled: true, yolo: true,
  precheck_command: 'git status --short', precheck_timeout_seconds: 30, is_running: false, scheduler_error: null, last_run: automationRun, next_run_at: '2026-09-12T01:00:00Z',
  created_at: '2026-09-11T01:00:00Z', updated_at: '2026-09-11T02:00:00Z' };
const automationSession = { ...session, id: 'codex:automation-session', session_id: 'automation-session', title: automation.name };

async function fixture(context, pins = { paths: [], revision: 0, failWrites: false }, terminalRows = 40, deferClaim = false) {
  const messages = [];
  const sockets = [];
  const mutations = [];
  const errors = [];
  const observedPaths = new Set();
  const agents = {};
  const titles = {};
  const terminalTabs = structuredClone(tabs);
  await context.route('**/api/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === '/api/aow/pinned-worktrees') {
      if (request.method() === 'PATCH') {
        if (pins.failWrites) { await route.fulfill({ status: 500, json: { message: 'Config write failed' } }); return; }
        const { add = [], remove = [], order } = request.postDataJSON();
        let next = [...new Set([...pins.paths.filter((path) => !remove.includes(path)), ...add])];
        if (order) next = [...new Set([...order.filter((path) => next.includes(path)), ...next])];
        if (JSON.stringify(next) !== JSON.stringify(pins.paths)) pins.revision += 1;
        pins.paths = next;
      }
      await route.fulfill({ json: { paths: pins.paths, revision: pins.revision } });
      return;
    }
    if (request.method() === 'POST' && url.pathname === '/api/git/ignored') {
      await route.fulfill({ json: { repository: workspace, ignored: [] } });
      return;
    }
    if (request.method() !== 'GET') { mutations.push(`${request.method()} ${url.pathname}`); await route.fulfill({ status: 405, json: { message: 'Unexpected mutation in browser test' } }); return; }
    let data;
    if (url.pathname === '/api/auth/status') data = { configured: true, authenticated: true };
    else if (url.pathname === '/api/aow/projects') data = [project];
    else if (url.pathname === '/api/aow/agents') data = [{ id: 'codex', display_name: 'Codex', available: true, args: [], env: {} }];
    else if (url.pathname === '/api/aow/settings') data = { notes_base: '/notes' };
    else if (url.pathname === '/api/terminals') data = terminalTabs.filter(tab => !url.searchParams.has('workspace_root') || tab.workspace_root === url.searchParams.get('workspace_root'));
    else if (url.pathname === '/api/terminals/agents') data = { agents, titles };
    else if (/^\/api\/terminals\/[^/]+$/.test(url.pathname)) {
      data = terminalTabs.find(tab => tab.id === decodeURIComponent(url.pathname.split('/').at(-1)));
      if (!data) { await route.fulfill({ status: 404, json: { message: 'Tab not found' } }); return; }
    }
    else if (url.pathname === '/api/aow/agent-sessions/automation-session/snapshot') data = { ...automationSession, captured_at: automationRun.finished_at, status: 'completed', truncated: false, turns: [{ id: 'automation-turn', status: 'completed', user: { text: automation.prompt, timestamp: automationRun.started_at }, final: { text: '## 本次巡检结果\n\n构建和测试均已通过。', timestamp: automationRun.finished_at } }] };
    else if (url.pathname.endsWith('/snapshot')) data = { ...session, captured_at: session.updated_at, status: 'completed', truncated: false, turns: [{ id: 'turn-1', status: 'completed', user: { text: '请实现 **手机端** UI', timestamp: session.created_at }, final: { text: '已完成。\n\n```ts\nconst mobile = true;\n```\n\n<img src=x onerror="window.__unsafe=true">', timestamp: session.updated_at } }] };
    else if (url.pathname === '/api/aow/agent-sessions') data = url.searchParams.get('agent') === 'codex' ? [session] : [];
    else if (url.pathname === '/api/aow/agent-sessions/automation-run') data = automationSession;
    else if (url.pathname.startsWith('/api/fs/tree')) {
      const dir = decodeURIComponent(url.pathname.slice('/api/fs/tree'.length));
      data = { path: dir, entries: [{ name: 'README.md', path: `${dir}/README.md`, kind: 'file', size: 4096 }, ...(dir.endsWith('/src') ? [] : [{ name: 'src', path: `${dir}/src`, kind: 'directory', size: 0 }])] };
    }
    else if (url.pathname.startsWith('/api/fs/raw')) {
      const path = decodeURIComponent(url.pathname.slice('/api/fs/raw'.length));
      if (path === '/tmp/not-a-file') { await route.fulfill({ status: 400, json: { message: 'path is not a regular file: /tmp/not-a-file' } }); return; }
      await route.fulfill({ body: 'external file', contentType: 'text/plain' });
      return;
    }
    else if (url.pathname.startsWith('/api/fs/text')) data = { path: workspace + '/README.md', content: '# AOW\n\n手机端 **文件预览**。\n\n<script>window.__unsafe=true</script>\n', language: 'markdown', mime: 'text/markdown', size: 100, version: 'v1' };
    else if (url.pathname === '/api/git/status') data = { repository: workspace, branch: 'main', ahead: 1, behind: 0, files: [{ path: 'frontend/src/main.tsx', index_status: 'M', worktree_status: 'M', original_path: null }] };
    else if (url.pathname === '/api/git/diff') data = { repository: workspace, path: 'frontend/src/main.tsx', staged: url.searchParams.get('staged') === 'true', patch: '@@ -1 +1 @@\n-desktop\n+mobile\n', truncated: false };
    else if (url.pathname === '/api/aow/automations/task-one/runs/run-one') data = automationRun;
    else if (url.pathname === '/api/aow/automations/task-one/runs/run-one/output/stdio') { await route.fulfill({ body: '构建和测试均已通过。', contentType: 'text/plain' }); return; }
    else if (url.pathname === '/api/aow/automations/task-one/runs') data = [automationRun];
    else if (url.pathname === '/api/aow/automations/task-one') data = automation;
    else if (url.pathname === '/api/aow/automations') data = [automation];
    else if (url.pathname === '/api/git/repositories') data = [{ name: 'AOW', path: workspace, branch: 'main' }];
    else if (url.pathname === '/api/git/ignored') data = { repository: workspace, ignored: [] };
    else if (url.pathname === '/api/git/log') data = { commits: [], repository: workspace, upstream: null, upstream_commit: null };
    else { await route.fulfill({ status: 404, json: { message: `No test fixture: ${url.pathname}` } }); return; }
    await route.fulfill({ json: data });
  });
  await context.routeWebSocket('**/api/terminals/**/ws?**', (socket) => {
    sockets.push(socket);
    socket.onMessage((message) => {
      messages.push({ url: socket.url(), message });
      const control = typeof message === 'string' ? JSON.parse(message) : undefined;
      if (!deferClaim && control?.type === 'claim') {
        const path = new URL(socket.url()).pathname;
        if (observedPaths.has(path) && !control.force) {
          socket.send(JSON.stringify({ type: 'control', state: 'observing' }));
          socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch-observer', offset: 0, reset: true, replay_bytes: 0,
            restore_cols: 120, restore_rows: terminalRows, restore: '\x1b[2J\x1b[HAow mobile terminal\r\n$ ' }));
          return;
        }
        if (control.force) observedPaths.delete(path);
        socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
        socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch-one', offset: 0, reset: true, replay_bytes: 0,
          restore_cols: 120, restore_rows: terminalRows, restore: '\x1b[2J\x1b[HAow mobile terminal\r\n$ ' }));
      }
    });
  });
  context.on('page', (page) => {
    page.setDefaultTimeout(10000);
    page.on('pageerror', (error) => errors.push(error.message));
  });
  if (process.env.MOBILE_TEST_FONT) {
    const font = await readFile(process.env.MOBILE_TEST_FONT);
    await context.route('**/__test-font.otf', (route) => route.fulfill({ body: font, contentType: 'font/otf' }));
    await context.addInitScript(() => {
      document.addEventListener('DOMContentLoaded', () => {
        const style = document.createElement('style');
        style.textContent = '@font-face{font-family:TestCJK;src:url(/__test-font.otf)}:root{font-family:Inter,TestCJK,sans-serif!important}';
        document.head.append(style);
      });
    });
  }
  return {
    messages, sockets, mutations, errors, agents, titles, terminalTabs,
    observeAfterSupersede(socket) { observedPaths.add(new URL(socket.url()).pathname); },
  };
}

async function visible(page, selector) { await page.locator(`${selector}:visible`).first().waitFor({ state: 'visible' }); }
async function terminalReady(page) {
  await page.waitForFunction(() => document.querySelector('.mobile-terminal-panel:not([hidden]) .mobile-composer textarea')?.disabled === false);
}
function controlMessages(state, type) {
  return state.messages.filter(({ message }) => typeof message === 'string').map(({ message }) => JSON.parse(message)).filter((message) => message.type === type);
}
function supersede(state, socket) {
  state.observeAfterSupersede(socket);
  socket.send(JSON.stringify({ type: 'error', code: 'attachment_superseded', message: '控制权已被其他窗口接管' }));
}
async function terminalSize(page, state, predicate = () => true) {
  for (let attempt = 0; attempt < 100; attempt++) {
    const size = controlMessages(state, 'resize').at(-1);
    if (size && predicate(size)) return size;
    await page.waitForTimeout(50);
  }
  assert.fail(`No matching terminal resize: ${JSON.stringify(controlMessages(state, 'resize'))}`);
}
async function terminalFits(page, fontSize = 12) {
  const viewport = await page.locator('.mobile-terminal-viewport:visible').boundingBox();
  const screen = await page.locator('.mobile-terminal-panel:not([hidden]) .xterm-screen').boundingBox();
  assert.ok(screen.width <= viewport.width + 1 && screen.height <= viewport.height + 1, 'terminal grid fits the available workspace');
  assert.ok(viewport.width - screen.width < 30 && viewport.height - screen.height < 20, 'terminal grid fills the workspace without shrinking desktop geometry');
  assert.equal(await page.locator('.mobile-terminal-panel:not([hidden]) .xterm-rows').evaluate((node) => getComputedStyle(node).fontSize), `${fontSize}px`);
}
async function snapshot(page, name) {
  if (!screenshots) return;
  await mkdir(screenshots, { recursive: true });
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: `${screenshots}/${name}.png`, fullPage: true });
}
async function noOverflow(page) {
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true, 'page must not overflow horizontally');
}

let browser;
try {
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('mobile terminal catalog opens worktree instances in the main workspace without duplicating terminals', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context);
    const feature = project.worktrees[1].path;
    state.terminalTabs.push(
      { id: 'remote-shell', name: '远程 Shell', workspace_root: feature, created_at: '2026-09-01T00:00:00Z', layout: leaf('remote-shell-pane'), panes: [pane('remote-shell-pane', 'Shell', { cwd: feature })] },
      { id: 'remote-cli', name: '远程 CLI', workspace_root: feature, layout: leaf('remote-cli-pane'), panes: [pane('remote-cli-pane', 'CLI', { cwd: feature, agent_id: 'codex', agent_terminal: { phase: 'ready' } })] },
      { id: 'unrelated', name: '其他项目实例', workspace_root: '/workspace/unrelated', layout: leaf('unrelated-pane'), panes: [pane('unrelated-pane', 'Shell')] },
    );
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    const header = page.locator('.mobile-header-actions');
    assert.deepEqual(await header.getByRole('button').evaluateAll(buttons => buttons.map(button => button.getAttribute('aria-label'))), ['终端列表', '刷新', '新建终端']);
    assert.equal(await page.locator('details.mobile-terminal-catalog').count(), 0);
    await header.getByRole('button', { name: '终端列表', exact: true }).click();
    const catalog = page.getByRole('dialog', { name: '终端列表', exact: true });
    await catalog.waitFor();
    assert.equal(await catalog.locator('.terminal-panel-row').count(), 2);
    const scope = async group => {
      await catalog.getByRole('button', { name: `${group} 显示范围`, exact: true }).click();
      await page.getByRole('menuitemcheckbox', { name: '显示所有Worktree' }).click();
    };
    await scope('User Terminals');
    await catalog.getByRole('button', { name: /远程 Shell/, exact: false }).first().waitFor();
    assert.equal(await catalog.locator('.terminal-panel-row').filter({ hasText: '远程 CLI' }).count(), 0, 'each terminal group has its own scope');
    await scope('CLI Terminals');
    const remoteCli = catalog.locator('.terminal-panel-row').filter({ hasText: '远程 CLI' });
    await remoteCli.locator('.terminal-panel-open').waitFor();
    assert.equal(await catalog.getByText('其他项目实例', { exact: true }).count(), 0);
    assert.equal(await page.locator('.mobile-terminal-tabs [role="tab"]').count(), 4, 'browsing does not add tabs');
    assert.equal(state.sockets.length, 0, 'listing all worktrees never attaches to their terminals');
    await catalog.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemradio', { name: '按创建时间' }).click();
    assert.match(await catalog.locator('.terminal-panel-row').first().textContent(), /远程 Shell/);
    await snapshot(page, 'mobile-terminal-catalog');
    await noOverflow(page);

    const remoteShell = catalog.locator('.terminal-panel-row').filter({ hasText: '远程 Shell' });
    await remoteShell.locator('.terminal-panel-open').click();
    await catalog.waitFor({ state: 'hidden' });
    await page.waitForURL('**/aow/tabs/terminal/remote-shell**');
    assert.equal(await page.locator('.mobile-brand-title span').textContent(), 'main', 'the host stays in the main workspace');
    assert.equal(await page.getByRole('tab', { name: '远程 Shell', exact: true }).getAttribute('aria-selected'), 'true');
    assert.equal(await page.getByRole('tab').count(), 5);
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    assert.match(state.sockets.at(-1).url(), /\/remote-shell\/panes\/remote-shell-pane\/ws/);
    const socketCount = state.sockets.length;
    await header.getByRole('button', { name: '终端列表', exact: true }).click();
    await remoteShell.getByRole('img', { name: '已显示', exact: true }).waitFor();
    await remoteShell.locator('.terminal-panel-open').click();
    await catalog.waitFor({ state: 'hidden' });
    assert.equal(await page.getByRole('tab').count(), 5);
    assert.equal(state.sockets.length, socketCount, 'opening the list and selecting the active instance keeps its connection');

    await header.getByRole('button', { name: '终端列表', exact: true }).click();
    await scope('User Terminals');
    await catalog.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.keyboard.press('Escape');
    await page.getByRole('menu', { name: '终端显示范围' }).waitFor({ state: 'hidden' });
    await catalog.waitFor();
    await page.keyboard.press('Escape');
    await catalog.waitFor({ state: 'hidden' });
    assert.equal(await page.getByRole('tab', { name: '远程 Shell', exact: true }).getAttribute('aria-selected'), 'true', 'filtering the catalog does not close an opened remote tab');
    assert.equal(await header.getByRole('button', { name: '终端列表', exact: true }).evaluate(button => button === document.activeElement), true);

    await header.getByRole('button', { name: '终端列表', exact: true }).click();
    await remoteCli.locator('.terminal-panel-open').click();
    await terminalReady(page);
    assert.match(state.sockets.at(-1).url(), /\/remote-cli\/panes\/remote-cli-pane\/ws/);
    assert.equal(await page.getByRole('tab').count(), 6);
    await page.getByRole('button', { name: '返回项目列表', exact: true }).click();
    await page.getByRole('button', { name: /main.*主目录/ }).click();
    await page.getByRole('tab', { name: '远程 CLI', exact: true }).waitFor();
    assert.equal(await page.getByRole('tab', { name: '远程 CLI', exact: true }).getAttribute('aria-selected'), 'true', 'returning restores hosted tabs and selection');
    assert.equal(await page.getByRole('tab', { name: '远程 Shell', exact: true }).count(), 1);
    await header.getByRole('button', { name: '终端列表', exact: true }).click();
    assert.equal(await catalog.locator('.terminal-panel-row').filter({ hasText: '远程 Shell' }).count(), 0, 'scope choice is remembered');
    await remoteCli.locator('.terminal-panel-open').waitFor();
    state.terminalTabs.splice(state.terminalTabs.findIndex(tab => tab.id === 'remote-cli'), 1);
    await catalog.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await remoteCli.waitFor({ state: 'hidden' });
    await catalog.getByRole('button', { name: '关闭终端列表' }).click();
    assert.equal(await page.getByRole('tab', { name: '远程 CLI', exact: true }).count(), 0);
    assert.equal(await page.getByRole('tab', { selected: true }).count(), 1, 'a removed remote instance falls back to an available tab');
    assert.deepEqual(state.mutations, []);
    assert.deepEqual(state.errors, []);
  });

  await test('mobile catalog remains available when empty and scopes non-main worktrees locally', async t => {
    const context = await browser.newContext({ viewport: { width: 320, height: 640 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(project.worktrees[1].path)}&view=terminal`);
    await page.getByRole('button', { name: '终端列表', exact: true }).click();
    const catalog = page.getByRole('dialog', { name: '终端列表' });
    assert.equal(await catalog.getByRole('button', { name: /显示范围/ }).count(), 0);
    assert.equal(await catalog.locator('.terminal-panel-row').count(), 0, 'other worktrees are not listed');
    const toggle = catalog.getByRole('button', { name: '展开 User Terminals', exact: true });
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false', 'empty panels start collapsed');
    await toggle.click();
    assert.equal(await catalog.getByRole('button', { name: '收起 User Terminals', exact: true }).getAttribute('aria-expanded'), 'true');
    await catalog.getByText('暂无终端', { exact: true }).first().waitFor();
    await noOverflow(page);
    await page.mouse.click(5, 200);
    await catalog.waitFor({ state: 'hidden' });
    assert.deepEqual(state.sockets, []);
    assert.deepEqual(state.mutations, []);
    assert.deepEqual(state.errors, []);
  });

  await test('mobile catalog rebuilds an interrupted active terminal and selects its new pane', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context);
    const old = state.terminalTabs.find(tab => tab.id === 'tab-b');
    old.panes[0].status = 'interrupted';
    let rebuilds = 0;
    await context.route('**/api/terminals/tab-b/rebuild', async route => {
      assert.equal(route.request().method(), 'POST');
      rebuilds += 1;
      old.panes[0] = { ...old.panes[0], id: 'rebuilt-server', status: 'running', exit_code: null };
      old.layout = leaf('rebuilt-server');
      old.revision += 1;
      await route.fulfill({ json: old });
    });
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('tab', { name: '服务', exact: true }).click();
    await page.getByRole('button', { name: '终端列表', exact: true }).click();
    const catalog = page.getByRole('dialog', { name: '终端列表', exact: true });
    const row = catalog.locator('.terminal-panel-row').filter({ hasText: '服务' });
    await row.getByRole('button', { name: '服务 终端操作', exact: true }).click();
    await page.getByRole('menuitem', { name: '重建', exact: true }).click();
    await row.getByText('运行中', { exact: true }).waitFor();
    await catalog.getByRole('button', { name: '关闭终端列表' }).click();
    assert.equal(await page.getByRole('tab', { name: '服务', exact: true }).getAttribute('aria-selected'), 'true');
    await page.locator('#mobile-terminal-panel-tab-b\\:rebuilt-server .terminal-emulator-shell:not(.restore-pending)').waitFor();
    assert.equal(rebuilds, 1);
    assert.deepEqual(state.mutations, []);
    assert.deepEqual(state.errors, []);
  });

  await test('mobile catalog retries project loading and confirms destruction inside the drawer', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context);
    state.terminalTabs.push({ id: 'remote-delete', name: '远程待销毁', workspace_root: project.worktrees[1].path,
      layout: leaf('delete-pane'), panes: [pane('delete-pane', 'Shell')] });
    let failList = true;
    let deletes = 0;
    await context.route('**/api/terminals**', async route => {
      const url = new URL(route.request().url());
      if (url.pathname === '/api/terminals' && !url.searchParams.has('workspace_root') && failList) {
        await route.fulfill({ status: 503, json: { message: '暂时无法加载项目终端' } });
      } else if (url.pathname === '/api/terminals/remote-delete' && route.request().method() === 'DELETE') {
        deletes += 1;
        state.terminalTabs.splice(state.terminalTabs.findIndex(tab => tab.id === 'remote-delete'), 1);
        await route.fulfill({ status: 204 });
      } else await route.fallback();
    });
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    await page.getByRole('button', { name: '终端列表', exact: true }).click();
    const catalog = page.getByRole('dialog', { name: '终端列表', exact: true });
    await catalog.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    const menu = page.getByRole('menu', { name: '终端显示范围' });
    const bounds = await menu.boundingBox();
    assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= 390 && bounds.y >= 0 && bounds.y + bounds.height <= 844);
    await menu.getByRole('menuitemcheckbox', { name: '显示所有Worktree' }).click();
    await catalog.getByRole('alert').filter({ hasText: '暂时无法加载项目终端' }).waitFor();
    assert.equal(await catalog.locator('.terminal-panel-row').count(), 2, 'local instances remain available after a project loading error');
    failList = false;
    await catalog.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    const row = catalog.locator('.terminal-panel-row').filter({ hasText: '远程待销毁' });
    await row.waitFor();
    assert.equal(await catalog.getByRole('alert').count(), 0);
    await row.getByRole('button', { name: '远程待销毁 终端操作', exact: true }).click();
    await page.getByRole('menuitem', { name: '销毁', exact: true }).click();
    const confirmation = page.getByRole('alertdialog', { name: '销毁终端？' });
    await confirmation.getByRole('button', { name: '取消', exact: true }).click();
    assert.equal(deletes, 0);
    await row.waitFor();
    await row.getByRole('button', { name: '远程待销毁 终端操作', exact: true }).click();
    await page.getByRole('menuitem', { name: '销毁', exact: true }).click();
    await confirmation.getByRole('button', { name: '销毁', exact: true }).click();
    await row.waitFor({ state: 'hidden' });
    assert.equal(deletes, 1);
    await catalog.waitFor();
    assert.deepEqual(state.sockets, []);
    assert.deepEqual(state.mutations, []);
    assert.deepEqual(state.errors, []);
  });

  await test('mobile terminal agent icons update per pane without taking terminal control', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    state.agents.shell = 'claude';
    state.agents.server = 'traecli';
    await page.waitForFunction(() => document.querySelector('[id="mobile-terminal-tab-a:shell"] img.agent-icon'));
    await page.waitForFunction(() => document.querySelector('[id="mobile-terminal-tab-b:server"] img.agent-icon'));
    assert.notEqual(await page.locator('[id="mobile-terminal-tab-a:shell"] img.agent-icon').getAttribute('src'),
      await page.locator('[id="mobile-terminal-tab-b:server"] img.agent-icon').getAttribute('src'));
    assert.equal(await page.locator('[id="mobile-terminal-tab-b:server"]').getAttribute('aria-selected'), 'false');
    state.agents.shell = null;
    state.agents.server = null;
    await page.waitForFunction(() => !document.querySelector('[id="mobile-terminal-tab-a:shell"] img.agent-icon'));
    await page.waitForFunction(() => !document.querySelector('[id="mobile-terminal-tab-b:server"] img.agent-icon'));
    assert.equal(state.sockets.length, 0);
    assert.deepEqual(state.errors, []);
    assert.deepEqual(state.mutations, []);
  });

  await test('mobile agent titles follow individual panes before any terminal attachment', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context);
    state.terminalTabs[0].name_is_custom = false;
    state.terminalTabs[1].name_is_custom = true;
    const customPane = state.terminalTabs[0].panes.find(pane => pane.id === 'logs');
    customPane.name_is_custom = true;
    state.agents.shell = 'codex';
    state.agents.codex = 'traecli';
    state.agents.server = 'claude';
    state.agents.logs = 'codex';
    state.titles.shell = '修复登录';
    state.titles.codex = '调整布局';
    state.titles.server = '服务端任务';
    state.titles.logs = '新的日志任务';
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('tab', { name: '修复登录', exact: true }).waitFor();
    await page.getByRole('tab', { name: '调整布局', exact: true }).waitFor();
    await page.getByRole('tab', { name: '服务端任务', exact: true }).waitFor();
    assert.equal(await page.getByRole('tab', { name: '新的日志任务', exact: true }).count(), 1);
    assert.equal(state.sockets.length, 0);
    state.agents.shell = null;
    await page.getByRole('tab', { name: '开发 · aow', exact: true }).waitFor();
    assert.equal(await page.getByRole('tab', { name: '调整布局', exact: true }).count(), 1);
    state.agents.server = null;
    await page.getByRole('tab', { name: '服务', exact: true }).waitFor();
    assert.deepEqual(state.mutations, []);
    assert.deepEqual(state.errors, []);
  });

  await test('mobile navigation, readers, back stack and desktop entry', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(baseURL);
    await visible(page, '.mobile-project-card');
    await snapshot(page, 'mobile-projects');
    await noOverflow(page);
    await page.getByRole('button', { name: /main.*主目录/ }).click();
    await visible(page, '.mobile-terminal-tabs');
    assert.deepEqual(await page.getByRole('tab').allTextContents(), ['开发 · aow', '开发 · aow', '开发 · aow', '服务']);
    assert.equal(state.sockets.length, 0, 'opening terminal list must not claim any terminal');
    assert.equal(await page.locator('.terminal-split').count(), 0);
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).isDisabled(), true);
    assert.equal(await page.locator('.mobile-bottom-nav').innerText(), '', 'bottom navigation uses icons with accessible labels');
    await page.locator('.mobile-app-header').getByRole('button', { name: '刷新', exact: true }).click();
    assert.equal(await page.locator('.mobile-app-header').getByRole('button', { name: '新建终端' }).isVisible(), true);
    const terminalBox = await page.locator('.mobile-terminal-viewport:visible').boundingBox();
    assert.ok(terminalBox.height >= 660, 'terminal gets the space freed by removing the zoom and scroll toolbar');
    assert.equal(await page.locator('.mobile-terminal-tools').count(), 0);
    await snapshot(page, 'mobile-terminal-before-claim');
    await page.getByRole('button', { name: 'Conversation', exact: true }).click();
    assert.equal(await page.locator('.mobile-header-actions button').count(), 0, 'terminal header actions disappear on other pages');
    await page.getByRole('button', { name: /实现手机端工作台/ }).click();
    await visible(page, '.mobile-message.assistant');
    assert.equal(await page.evaluate(() => window.__unsafe), undefined, 'transcript HTML must be sanitized');
    await snapshot(page, 'mobile-conversation');
    await page.goBack();
    await visible(page, '.mobile-session-row');
    await page.getByRole('button', { name: '文件', exact: true }).click();
    await page.getByRole('button', { name: /src 文件夹/ }).click();
    await page.getByRole('button', { name: /README.md/ }).click();
    await visible(page, '.mobile-file-reader h1');
    assert.equal(await page.evaluate(() => window.__unsafe), undefined);
    await page.getByRole('button', { name: '返回', exact: true }).click();
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/files');
    assert.equal(new URL(page.url()).searchParams.get('path'), workspace + '/src');
    await visible(page, '.mobile-list-row');
    await page.getByRole('tab', { name: 'Notes' }).click();
    await page.getByRole('button', { name: /README.md/ }).click();
    assert.equal(new URL(page.url()).pathname, '/aow/tabs/file');
    assert.equal(new URL(page.url()).searchParams.get('source'), 'notes');
    await visible(page, '.mobile-file-reader h1');
    await page.getByRole('button', { name: 'Git', exact: true }).click();
    await page.getByRole('button', { name: /已暂存.*查看 Diff/ }).click();
    await visible(page, '.mobile-diff .added');
    assert.match(await page.locator('.mobile-diff').innerText(), /mobile/);
    await page.getByRole('button', { name: '自动化', exact: true }).click();
    await page.getByRole('button', { name: /每日代码巡检/ }).click();
    await visible(page, '.mobile-automation-details');
    assert.match(await page.locator('.mobile-automation-details').innerText(), /任务 ID\s*task-one/);
    assert.match(await page.locator('.mobile-automation-details').innerText(), /执行权限\s*Yolo \/ Full Access/);
    assert.match(await page.locator('.mobile-automation-markdown').innerText(), /巡检要求/);
    await page.getByRole('button', { name: /已完成/ }).click();
    await visible(page, '.mobile-automation-result article');
    assert.match(await page.locator('.mobile-automation-details').innerText(), /Run ID\s*run-one/);
    assert.match(await page.locator('.mobile-automation-result').innerText(), /本次巡检结果/);
    assert.doesNotMatch(await page.locator('.mobile-automation-result').innerText(), /巡检要求/);
    await noOverflow(page);
    await page.getByRole('button', { name: '返回项目列表' }).click();
    await page.getByRole('link', { name: '打开桌面版' }).click();
    await visible(page, '.project-aow');
    assert.equal(await page.locator('.mobile-app').count(), 0);
    assert.deepEqual(state.errors, []);
    assert.deepEqual(state.mutations, []);
    await context.close();
  });

  await test('terminal control, bytes, responsive geometry, keyboard and pane switching', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    assert.equal(state.sockets.length, 1);
    assert.match(await page.locator('.terminal-emulator').innerText(), /AOW mobile terminal/);
    assert.equal(JSON.parse(state.messages[0].message).force, false, 'first mobile attach observes before an explicit takeover');
    const portrait = await terminalSize(page, state, (size) => size.cols < 60 && size.rows > 35);
    await terminalFits(page);
    const keys = [['Tab', '\t'], ['Enter', '\r'], ['Shift+Tab', '\x1b[Z'], ['Space', ' '], ['Ctrl+C', '\x03']];
    for (const [name] of keys) await page.getByRole('button', { name: `发送 ${name}`, exact: true }).click();
    await page.waitForTimeout(100);
    let inputMessages = state.messages.filter((entry) => typeof entry.message !== 'string').map((entry) => Buffer.from(entry.message).toString());
    assert.deepEqual(inputMessages, keys.map(([, bytes]) => bytes));
    await page.getByRole('textbox', { name: '终端命令' }).fill('echo 手机端');
    await page.getByRole('button', { name: '执行命令', exact: true }).click();
    await page.waitForTimeout(100);
    inputMessages = state.messages.filter((entry) => typeof entry.message !== 'string').map((entry) => Buffer.from(entry.message).toString());
    assert.deepEqual(inputMessages.slice(-2), ['echo 手机端', '\r']);
    await page.getByRole('textbox', { name: '终端命令' }).fill('do not execute this');
    await page.getByRole('button', { name: '发送 Ctrl+C', exact: true }).click();
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).inputValue(), 'do not execute this');
    await page.getByRole('textbox', { name: '终端命令' }).fill('');
    await page.getByRole('button', { name: '收起键盘', exact: true }).click();
    await page.setViewportSize({ width: 844, height: 390 });
    await terminalSize(page, state, (size) => size.cols > portrait.cols && size.rows < portrait.rows);
    await terminalFits(page);
    await page.setViewportSize({ width: 390, height: 844 });
    await terminalSize(page, state, (size) => size.cols === portrait.cols && size.rows === portrait.rows);
    const resizeCount = controlMessages(state, 'resize').length;
    const canvas = await page.locator('.mobile-terminal-stage:visible').boundingBox();
    const navigationButtonCount = await page.locator('.mobile-bottom-nav button').count();
    const navigationHeight = (await page.locator('.mobile-bottom-nav').boundingBox()).height;
    await page.getByRole('textbox', { name: '终端命令' }).focus();
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), true, 'input focus alone does not hide navigation without a soft keyboard');
    await page.setViewportSize({ width: 390, height: 460 });
    await page.waitForTimeout(350);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'Android keyboard resizing the layout viewport does not resize the PTY');
    assert.equal((await page.locator('.mobile-terminal-stage:visible').boundingBox()).height, canvas.height);
    const keyboardBox = await page.locator('.mobile-terminal-viewport:visible').boundingBox();
    assert.ok(keyboardBox.height >= 280, 'terminal keeps more than half of the viewport above the keyboard');
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), false, 'terminal navigation hides while the keyboard is open');
    assert.equal(await page.getByRole('navigation', { name: '工作区导航' }).count(), 0, 'hidden navigation cannot be focused or activated');
    const dismiss = page.getByRole('button', { name: '收起键盘' });
    assert.equal(await dismiss.isVisible(), true);
    assert.equal(await page.locator('.mobile-bottom-nav button').count(), navigationButtonCount, 'bottom navigation has no additional keyboard button');
    assert.equal(await dismiss.locator('.lucide-keyboard-off').count(), 1, 'the composer button changes its icon when focused');
    const inputBox = await page.locator('.mobile-terminal-input:visible').boundingBox();
    assert.equal(inputBox.y + inputBox.height, 460, 'terminal input fills the space released by hiding navigation');
    const composerBox = await page.locator('.mobile-composer:visible').boundingBox();
    assert.ok(composerBox.y + composerBox.height <= 460, 'command input stays above the keyboard');
    await snapshot(page, 'mobile-terminal-keyboard');
    await page.getByRole('button', { name: '收起键盘', exact: true }).click();
    assert.equal(await dismiss.isVisible(), false);
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).evaluate((node) => node === document.activeElement), false);
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), false, 'navigation stays hidden during keyboard dismissal');
    assert.equal(await page.getByRole('button', { name: '打开键盘' }).locator('.lucide-keyboard').count(), 1);
    await page.waitForTimeout(150);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'blur does not resize during keyboard dismissal');
    await page.setViewportSize({ width: 390, height: 844 });
    await terminalSize(page, state, (size) => size.cols === portrait.cols && size.rows === portrait.rows);
    await terminalFits(page);
    await page.waitForTimeout(250);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'keyboard open and close leave terminal dimensions unchanged');
    assert.equal((await page.locator('.mobile-bottom-nav').boundingBox()).height, navigationHeight, 'navigation returns at its original height after keyboard dismissal');
    await snapshot(page, 'mobile-terminal-controlled');
    await page.getByRole('textbox', { name: '终端命令' }).fill('keep this draft');
    await page.getByRole('button', { name: '收起键盘', exact: true }).click();
    await page.locator('[id="mobile-terminal-tab-a:codex"]').click();
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    assert.equal(state.sockets.length, 2);
    assert.match(state.sockets[1].url(), /panes\/codex\/ws/);
    supersede(state, state.sockets[1]);
    await page.waitForFunction(() => document.querySelector('.mobile-terminal-panel:not([hidden]) .mobile-composer textarea')?.disabled);
    const mutationCount = () => state.messages.filter(({ message }) => typeof message !== 'string' || JSON.parse(message).type === 'resize').length;
    const count = mutationCount();
    await page.locator('.mobile-terminal-panel:not([hidden]) .mobile-composer textarea').evaluate((node) => { node.value = 'blocked'; node.dispatchEvent(new Event('input', { bubbles: true })); });
    await page.setViewportSize({ width: 844, height: 390 });
    await page.waitForTimeout(400);
    assert.equal(mutationCount(), count, 'superseded and inactive panes cannot send input or resize');
    await page.locator('[id="mobile-terminal-tab-a:shell"]').click();
    await terminalReady(page);
    assert.equal(controlMessages(state, 'claim').at(-1).force, false, 'revisiting a pane must not force takeover');
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).inputValue(), 'keep this draft');
    await terminalSize(page, state, (size) => size.cols > portrait.cols && size.rows < portrait.rows);
    await terminalFits(page);
    assert.deepEqual(state.mutations, [], 'mobile tab flattening must never update desktop layout');
    await noOverflow(page);
    assert.deepEqual(state.errors, []);
    await context.close();
  });
  await test('direct terminal soft keyboard sends spaces, punctuation and IME text exactly once', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    const textarea = page.locator('.mobile-terminal-panel:not([hidden]) .xterm-helper-textarea');
    const inputs = () => state.messages.filter(({ message }) => typeof message !== 'string').map(({ message }) => Buffer.from(message).toString()).join('');
    const insert = async (text, keyCode = 0, delayed = false) => {
      for (const data of text) {
        await textarea.evaluate(async (node, { data, keyCode, delayed }) => {
          const key = { key: keyCode === 229 ? 'Unidentified' : data, keyCode, bubbles: true, cancelable: true, composed: true };
          node.dispatchEvent(new KeyboardEvent('keydown', key));
          if (delayed) await new Promise((resolve) => setTimeout(resolve, 30));
          node.dispatchEvent(new InputEvent('beforeinput', { inputType: 'insertText', data, bubbles: true, composed: true }));
          node.value += data;
          node.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data, bubbles: true, composed: true }));
          node.dispatchEvent(new KeyboardEvent('keyup', key));
        }, { data, keyCode, delayed });
        await page.waitForTimeout(20);
      }
    };
    await insert('blocked !');
    assert.equal(inputs(), '', 'direct input still requires takeover');
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    await textarea.focus();
    await page.keyboard.type('echo 123');
    const symbols = ' !"#$%&\'()*+,-./:;<=>?@[\\]^_`{|}~';
    await insert(symbols);
    assert.equal(inputs(), `echo 123${symbols}`, 'soft keyboards can insert text without a keypress or a usable legacy keyCode');
    await insert(' ', 32);
    await insert('，。！？', 229);
    await insert(' -_./', 229, true);
    await page.keyboard.insertText('🙂');
    await page.waitForTimeout(30);
    const prefix = `echo 123${symbols} ，。！？ -_./🙂`;
    assert.equal(inputs(), prefix, 'IME keyCode 229 and input-only keyboards do not duplicate text');
    await textarea.evaluate((node) => {
      node.value = '';
      node.dispatchEvent(new KeyboardEvent('keydown', { key: 'Unidentified', keyCode: 229, bubbles: true }));
      node.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      node.dispatchEvent(new CompositionEvent('compositionupdate', { data: '中文', bubbles: true }));
      node.value = '中文';
      node.dispatchEvent(new InputEvent('input', { data: '中文', inputType: 'insertCompositionText', isComposing: true, composed: true, bubbles: true }));
    });
    await page.waitForTimeout(30);
    assert.equal(inputs(), prefix, 'uncommitted composition must not be sent');
    await textarea.evaluate((node) => {
      node.dispatchEvent(new CompositionEvent('compositionend', { data: '中文', bubbles: true }));
      node.dispatchEvent(new KeyboardEvent('keyup', { key: 'Unidentified', keyCode: 229, bubbles: true }));
    });
    await page.waitForTimeout(30);
    await insert(' !');
    for (const inputType of ['deleteContentBackward', 'insertLineBreak']) {
      await textarea.evaluate(async (node, inputType) => {
        const key = { key: 'Unidentified', keyCode: 229, bubbles: true, composed: true };
        node.dispatchEvent(new KeyboardEvent('keydown', key));
        await new Promise((resolve) => setTimeout(resolve, 30));
        const edit = new InputEvent('beforeinput', { inputType, bubbles: true, cancelable: true, composed: true });
        if (node.dispatchEvent(edit)) {
          node.value = inputType === 'deleteContentBackward' ? node.value.slice(0, -1) : `${node.value}\n`;
          node.dispatchEvent(new InputEvent('input', { inputType, bubbles: true, composed: true }));
        }
        node.dispatchEvent(new KeyboardEvent('keyup', key));
      }, inputType);
    }
    await page.keyboard.press('Backspace');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Control+c');
    await page.waitForTimeout(30);
    assert.equal(inputs(), `${prefix}中文 !\x7f\r\x7f\r\x03`, 'composition, subsequent symbols and soft/physical control keys remain distinct');
    await textarea.evaluate((node) => {
      for (let index = 0; index < 2; index++) {
        node.dispatchEvent(new InputEvent('beforeinput', { inputType: 'deleteContentBackward', bubbles: true, cancelable: true, composed: true }));
      }
    });
    await page.waitForTimeout(30);
    assert.equal(inputs(), `${prefix}中文 !\x7f\r\x7f\r\x03\x7f\x7f`, 'input-only keyboards can repeatedly backspace even with an empty helper textarea');
    supersede(state, state.sockets[0]);
    await page.waitForFunction(() => document.querySelector('.mobile-composer textarea')?.disabled);
    const sent = inputs();
    await insert('blocked !');
    assert.equal(inputs(), sent, 'revoked direct input cannot reach the PTY');
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('keyboard button tracks native shell focus and buffered composer input', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    await terminalSize(page, state);
    const viewport = page.locator('.mobile-terminal-viewport:visible');
    const helper = page.locator('.mobile-terminal-panel:not([hidden]) .xterm-helper-textarea');
    const composer = page.getByRole('textbox', { name: '终端命令' });
    const focused = (node) => node.evaluate((element) => element === document.activeElement);
    const tap = () => viewport.tap({ position: { x: 100, y: 100 } });
    // Exercise xterm's native mouse handler, independently of our scroll/pinch gestures.
    const activateShell = () => page.locator('.mobile-terminal-panel:not([hidden]) .xterm').dispatchEvent('mousedown', { button: 0 });
    const inputs = () => state.messages.filter(({ message }) => typeof message !== 'string').map(({ message }) => Buffer.from(message).toString());
    const dismiss = page.getByRole('button', { name: '收起键盘', exact: true });
    const open = page.getByRole('button', { name: '打开键盘', exact: true });
    await page.waitForTimeout(150);
    const resizeCount = controlMessages(state, 'resize').length;
    const stage = await page.locator('.mobile-terminal-stage:visible').boundingBox();
    await activateShell();
    assert.equal(await focused(helper), true, 'xterm still activates direct shell input through its native handler');
    assert.equal(await dismiss.locator('.lucide-keyboard-off').count(), 1, 'direct shell focus updates the keyboard icon');
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), true, 'direct shell focus without a soft keyboard keeps navigation visible');
    await page.setViewportSize({ width: 390, height: 460 });
    await page.waitForTimeout(200);
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), false, 'direct shell input also hides navigation when the keyboard opens');
    await tap();
    await page.waitForTimeout(150);
    assert.equal(await focused(helper), true, 'tapping the terminal again keeps direct input active');
    assert.equal(await dismiss.isVisible(), true);
    await dismiss.tap();
    await page.waitForTimeout(150);
    assert.equal(await focused(helper), false, 'the keyboard button blurs direct shell input');
    assert.equal(await focused(composer), false, 'hiding the keyboard must not focus the composer');
    assert.equal(await open.locator('.lucide-keyboard').count(), 1, 'the icon returns to open after dismissal');
    assert.equal((await page.locator('.mobile-terminal-stage:visible').boundingBox()).height, stage.height);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'dismissal does not resize the PTY during the closing animation');
    await page.setViewportSize({ width: 390, height: 844 });
    await page.waitForTimeout(250);
    assert.equal(controlMessages(state, 'resize').length, resizeCount);
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), true, 'navigation returns after dismissing direct shell input');
    await open.tap();
    assert.equal(await focused(composer), true, 'the keyboard button can still open the composer');
    await composer.fill('keep this draft');
    assert.equal(await dismiss.locator('.lucide-keyboard-off').count(), 1);
    await activateShell();
    assert.equal(await focused(composer), false);
    assert.equal(await focused(helper), true, 'tapping the terminal transfers composer focus into direct input');
    assert.equal(await dismiss.isVisible(), true, 'transferring input keeps the icon in its dismiss state');
    assert.equal(await composer.inputValue(), 'keep this draft');
    await dismiss.tap();
    assert.equal(await focused(helper), false);
    await open.tap();
    await dismiss.tap();
    assert.equal(await focused(composer), false, 'the same keyboard button dismisses composer input');
    assert.equal(await composer.inputValue(), 'keep this draft');
    assert.deepEqual(inputs(), [], 'keyboard toggling does not execute the draft or send input');
    await activateShell();
    await page.keyboard.insertText('direct shell text');
    await page.waitForTimeout(50);
    assert.deepEqual(inputs(), ['direct shell text'], 'direct input is sent immediately');
    await composer.fill('echo buffered input');
    assert.deepEqual(inputs(), ['direct shell text'], 'composer text stays local until submitted');
    await page.getByRole('button', { name: '执行命令', exact: true }).click();
    await page.waitForTimeout(50);
    assert.deepEqual(inputs(), ['direct shell text', 'echo buffered input', '\r'], 'the composer submits its complete draft once');
    assert.equal(await composer.inputValue(), '');
    await activateShell();
    assert.equal(await focused(helper), true, 'native shell input can be reactivated');
    const cdp = await context.newCDPSession(page);
    const box = await viewport.boundingBox();
    const point = { x: box.x + 100, y: box.y + 100, id: 0 };
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [point] });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchCancel', touchPoints: [] });
    assert.equal(await focused(helper), true, 'cancelled touches do not dismiss the keyboard');
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [point] });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ ...point, y: point.y + 40 }] });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    assert.equal(await focused(helper), true, 'scroll gestures preserve direct input focus');
    await tap();
    assert.equal(await focused(helper), true, 'a tap after scrolling also keeps editing');
    await dismiss.tap();
    assert.equal(await focused(helper), false);
    assert.equal(state.sockets.length, 1, 'keyboard toggling does not reconnect the terminal');
    await activateShell();
    supersede(state, state.sockets[0]);
    await page.waitForFunction(() => document.querySelector('.mobile-composer textarea')?.disabled);
    assert.equal(await focused(helper), false, 'losing control also releases direct input focus');
    assert.equal(await open.isDisabled(), true, 'the keyboard icon resets when control is lost');
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('keyboard focus and dismissal preserve terminal content rows', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    await terminalSize(page, state);
    const terminal = page.locator('.mobile-terminal-panel:not([hidden]) .xterm');
    const rows = terminal.locator('.xterm-rows');
    state.sockets[0].send(Buffer.from('\x1b[2J\x1b[H' + Array.from({ length: 15 }, (_, i) => `stable output ${i}`).join('\r\n') + '\r\n$ '));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('stable output 14'));
    await page.waitForTimeout(200);
    const resizeCount = controlMessages(state, 'resize').length;
    await rows.evaluate((element) => {
      const changed = new Set();
      const observer = new MutationObserver((records) => {
        for (const record of records) {
          if (record.target.parentElement === element) changed.add(Array.from(element.children).indexOf(record.target));
        }
      });
      observer.observe(element, { childList: true, subtree: true });
      window.__terminalPaint = { changed, observer };
    });
    const paintedRows = async () => {
      await page.waitForTimeout(100);
      return page.evaluate(() => {
        const rows = [...window.__terminalPaint.changed];
        window.__terminalPaint.changed.clear();
        return rows;
      });
    };
    const box = await rows.boundingBox();
    const click = { button: 0, detail: 1, clientX: box.x + 20, clientY: box.y + 10 };
    await terminal.dispatchEvent('mousedown', click);
    await terminal.dispatchEvent('mouseup', click);
    const opening = await paintedRows();
    assert.equal(await rows.locator('.xterm-cursor-block').count(), 1, 'direct input still paints a focused cursor');
    await page.setViewportSize({ width: 390, height: 460 });
    const shrinking = await paintedRows();
    await page.getByRole('button', { name: '收起键盘', exact: true }).tap();
    const closing = await paintedRows();
    assert.equal(await rows.locator('.xterm-cursor-outline').count(), 1, 'dismissing input still paints an unfocused cursor');
    await page.setViewportSize({ width: 390, height: 844 });
    const expanding = await paintedRows();
    assert.ok(opening.length <= 1, `shell focus only repaints the cursor row; repainted ${opening.length} rows`);
    assert.ok(closing.length <= 1, `shell blur only repaints the cursor row; repainted ${closing.length} rows`);
    assert.deepEqual(shrinking, [], 'keyboard opening does not rebuild terminal content');
    assert.deepEqual(expanding, [], 'keyboard closing does not rebuild terminal content');
    await page.getByRole('button', { name: '打开键盘', exact: true }).tap();
    await page.setViewportSize({ width: 390, height: 460 });
    assert.deepEqual(await paintedRows(), [], 'composer input does not repaint terminal content');
    await page.getByRole('button', { name: '收起键盘', exact: true }).tap();
    await page.setViewportSize({ width: 390, height: 844 });
    assert.deepEqual(await paintedRows(), [], 'composer dismissal does not repaint terminal content');
    assert.equal(controlMessages(state, 'resize').length, resizeCount);
    assert.equal(state.sockets.length, 1);
    const plainRow = await rows.locator(':scope > div').first().innerHTML();
    await terminal.dispatchEvent('mousedown', { ...click, detail: 2 });
    await terminal.dispatchEvent('mouseup', { ...click, detail: 2 });
    await page.waitForFunction(() => document.querySelector('.xterm-selection')?.childElementCount > 0);
    const selectedRow = await rows.locator(':scope > div').first().innerHTML();
    assert.notEqual(selectedRow, plainRow, 'selecting text still paints its highlight');
    await paintedRows();
    await page.getByRole('button', { name: '收起键盘', exact: true }).tap();
    assert.ok((await paintedRows()).length > 1, 'a real selection retains the normal inactive-color repaint');
    assert.equal(await rows.evaluate((node) => node.classList.contains('xterm-focus')), false);
    // This theme uses the same selection color while focused and blurred.
    assert.equal(await rows.locator(':scope > div').first().innerHTML(), selectedRow, 'the selected text remains highlighted after blur');
    await terminal.dispatchEvent('mousedown', click);
    await terminal.dispatchEvent('mouseup', click);
    await page.waitForFunction(() => document.querySelector('.xterm-selection')?.childElementCount === 0);
    assert.equal(await rows.locator(':scope > div').first().innerHTML(), plainRow, 'clearing a selection removes its cell styling');
    await page.getByRole('button', { name: '收起键盘', exact: true }).tap();
    await paintedRows();
    state.sockets[0].send(Buffer.from('live output'));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('$ live output'));
    assert.ok((await paintedRows()).length > 0, 'live terminal output still renders');
    await page.evaluate(() => window.__terminalPaint.observer.disconnect());
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('touch and wheel scroll terminal history and TUI contents', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    const size = await terminalSize(page, state);
    const socket = state.sockets[0];
    const inputs = () => state.messages.filter(({ message }) => typeof message !== 'string').map(({ message }) => Buffer.from(message).toString());
    socket.send(Buffer.from(Array.from({ length: 200 }, (_, index) => `NORMAL-${index}\r\n`).join('')));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('NORMAL-199'));
    const rows = page.locator('.xterm-rows');
    const before = await rows.innerText();
    const cdp = await context.newCDPSession(page);
    const screen = await page.locator('.xterm-screen').boundingBox();
    const x = screen.x + screen.width / 2;
    const y = screen.y + 60;
    const swipe = async (dx, dy) => {
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y, id: 0 }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x + dx / 2, y: y + dy / 2, id: 0 }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x + dx, y: y + dy, id: 0 }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
      await page.waitForTimeout(100);
    };
    const inputCount = inputs().length;
    await swipe(0, 90);
    assert.notEqual(await rows.innerText(), before, 'single-finger drag scrolls actual terminal history');
    assert.equal(await page.locator('.mobile-terminal-viewport').evaluate((node) => node.scrollTop), 0, 'history scroll does not merely move the canvas');
    const afterTouch = await rows.innerText();
    await page.mouse.move(x, y);
    await page.mouse.wheel(0, -100);
    await page.waitForTimeout(100);
    assert.notEqual(await rows.innerText(), afterTouch, 'external wheel also scrolls terminal history');
    assert.equal(inputs().length, inputCount, 'normal history scrolling never sends commands');
    await snapshot(page, 'mobile-terminal-history');
    await page.mouse.wheel(0, 10000);
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('NORMAL-199'));

    socket.send(Buffer.from('\x1b[?1049h\x1b[?1h\x1b[2J\x1b[HTUI-ALTERNATE'));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('TUI-ALTERNATE'));
    const altStart = inputs().length;
    await swipe(0, -35);
    assert.ok(inputs().length > altStart, 'alternate-screen scroll reaches the application');
    assert.ok(inputs().slice(altStart).every((data) => data === '\x1bOB'), 'alternate screen obeys application cursor mode');
    socket.send(Buffer.from('\x1b[?1049l\x1b[?1000h\x1b[?1006h\r\nTUI-MOUSE'));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('TUI-MOUSE'));
    const mouseStart = inputs().length;
    await swipe(0, 35);
    const mouseInput = inputs().slice(mouseStart);
    assert.ok(mouseInput.length > 0);
    assert.ok(mouseInput.every((data) => /^\x1b\[<64;\d+;\d+M$/.test(data)), 'mouse-aware applications receive SGR scroll even on the normal buffer');
    const last = mouseInput.at(-1).match(/^\x1b\[<64;(\d+);(\d+)M$/);
    assert.ok(Math.abs(Number(last[1]) - (Math.floor(size.cols / 2) + 1)) <= 1, 'mouse coordinates match the fitted mobile grid');
    const expectedRow = Math.floor((y + 35 - screen.y) / screen.height * size.rows) + 1;
    assert.ok(Math.abs(Number(last[2]) - expectedRow) <= 1);
    const pinchStart = inputs().length;
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: x - 30, y, id: 0 }, { x: x + 30, y, id: 1 }] });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x - 40, y: y + 10, id: 0 }, { x: x + 40, y: y + 10, id: 1 }] });
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    const zoomed = await terminalSize(page, state, (next) => next.cols < size.cols && next.rows < size.rows);
    assert.equal(inputs().length, pinchStart, 'font zoom must not send TUI input');
    supersede(state, socket);
    await page.waitForFunction(() => document.querySelector('.mobile-composer textarea')?.disabled);
    await page.locator('.mobile-terminal-viewport').dispatchEvent('wheel', { deltaY: -100, clientX: x, clientY: y, bubbles: true, cancelable: true });
    assert.equal(inputs().length, pinchStart, 'revoked control cannot send scroll input');
    assert.deepEqual(controlMessages(state, 'resize').at(-1), zoomed, 'revoked scroll leaves the terminal size unchanged');
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('pinch changes terminal font size, refits the grid and remembers each pane', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 2 });
    t.after(() => context.close());
    await context.addInitScript(() => {
      if (!['http:', 'https:'].includes(location.protocol)) return;
      const key = 'aow.mobile.font-size.tab-a:shell';
      if (!sessionStorage.getItem(key)) sessionStorage.setItem(key, '13');
    });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    const original = await terminalSize(page, state);
    const cdp = await context.newCDPSession(page);
    const pinch = async (start, end) => {
      const box = await page.locator('.mobile-terminal-viewport:visible').boundingBox();
      const x = box.x + box.width / 2;
      const y = box.y + 100;
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: x - start / 2, y, id: 0 }, { x: x + start / 2, y, id: 1 }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x - end / 2, y, id: 0 }, { x: x + end / 2, y, id: 1 }] });
      // Moving the remaining finger after a pinch must not scroll or send arrow keys.
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [{ x: x - end / 2, y, id: 0 }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: x - end / 2, y: y + 50, id: 0 }] });
      await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
    };
    await page.getByRole('textbox', { name: '终端命令' }).fill('unsent draft');
    const nav = page.getByRole('navigation', { name: '工作区导航' });
    const before = await nav.boundingBox();
    await pinch(100, 150);
    await terminalSize(page, state, (size) => size.cols < original.cols && size.rows < original.rows);
    await terminalFits(page, 18);
    assert.deepEqual(await nav.boundingBox(), before, 'font zoom does not scale the navigation or the page');
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).evaluate((node) => document.activeElement === node), true, 'pinch preserves keyboard focus');
    assert.equal(state.sockets.length, 1, 'font changes do not reconnect the terminal');
    await snapshot(page, 'mobile-terminal-font-zoom');
    await pinch(150, 100);
    await terminalSize(page, state, (size) => size.cols === original.cols && size.rows === original.rows);
    await terminalFits(page);
    // Repeated round trips must not accumulate rounding drift in the PTY grid.
    await pinch(100, 150);
    await terminalSize(page, state, (size) => size.cols < original.cols);
    await pinch(150, 100);
    await terminalSize(page, state, (size) => size.cols === original.cols && size.rows === original.rows);
    await terminalFits(page);
    await pinch(100, 150);
    await terminalSize(page, state, (size) => size.cols < original.cols);
    await page.getByRole('button', { name: '收起键盘' }).click();
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).inputValue(), 'unsent draft');
    assert.equal(await page.getByRole('button', { name: '收起键盘' }).isVisible(), false);
    assert.equal(state.messages.filter(({ message }) => typeof message !== 'string').length, 0, 'zoom and keyboard dismissal do not execute the draft');
    await page.locator('[id="mobile-terminal-tab-a:codex"]').click();
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    await terminalFits(page);
    await page.locator('[id="mobile-terminal-tab-a:shell"]').click();
    await terminalReady(page);
    await terminalFits(page, 18);
    await page.reload();
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    await terminalFits(page, 18);
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).inputValue(), 'unsent draft');
    await noOverflow(page);
    assert.deepEqual(state.errors, []);
  });

  await test('mobile CLI lifecycle and connection messages match the terminal catalog', async t => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context, undefined, 40, true);
    const cliPane = pane('cli-state', 'CLI state', { agent_terminal: { phase: 'starting', error: null } });
    state.terminalTabs.push({ id: 'cli-state-tab', name: 'CLI state', workspace_root: workspace, revision: 1,
      layout: leaf(cliPane.id), panes: [cliPane] });
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.getByRole('button', { name: '终端列表', exact: true }).click();
    const row = page.locator('.terminal-panel-row').filter({ hasText: 'CLI state' });
    await row.locator('.terminal-panel-open').click();
    await page.waitForFunction(() => document.querySelector('.mobile-terminal-panel:not([hidden]) .terminal-connection')?.classList.contains('connecting'));
    const welcome = page.locator('.mobile-terminal-panel:not([hidden]) .mobile-terminal-welcome');
    assert.equal(await welcome.getByRole('status').textContent(), '初始化中 · 连接中');
    const dot = page.locator('[id="mobile-terminal-cli-state-tab:cli-state"] .mobile-dot');
    await page.getByRole('button', { name: '终端列表', exact: true }).click();
    await row.getByRole('img', { name: '已显示', exact: true }).waitFor();
    assert.equal(await dot.evaluate(el => getComputedStyle(el).backgroundColor),
      await row.locator('.terminal-panel-status-dot').evaluate(el => getComputedStyle(el).backgroundColor));
    await page.waitForFunction(() => document.querySelector('.mobile-terminal-panel:not([hidden]) .terminal-restore-status') !== null);
    assert.equal(state.sockets.length, 1);
    const socket = state.sockets[0];
    socket.send(JSON.stringify({ type: 'control', state: 'observing' }));
    socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch-observer', offset: 0, reset: true, replay_bytes: 0,
      restore_cols: 120, restore_rows: 40, restore: '\x1b[2J\x1b[Hobserver $ ' }));
    await row.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    socket.send(JSON.stringify({ type: 'control', state: 'invalid' }));
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.equal(await welcome.locator('[role="status"]').textContent(), '初始化中 · 服务器返回了无效的终端控制状态');
    cliPane.agent_terminal.phase = 'failed';
    await page.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await page.locator('[id="mobile-terminal-cli-state-tab:cli-state"] .mobile-dot.failed').waitFor();
    assert.match(await welcome.locator('[role="status"]').textContent(), /^启动失败 · 服务器返回了无效的终端控制状态/);
    cliPane.status = 'exited';
    await page.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await page.locator('[id="mobile-terminal-cli-state-tab:cli-state"] .mobile-dot.exited').waitFor();
    assert.equal(await row.locator('.terminal-panel-status-label').textContent(), '已退出');
    assert.deepEqual(state.errors, []);
  });

  await test('terminal observer renders output without enabling input until explicit takeover', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const state = await fixture(context, undefined, 40, true);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    const socket = state.sockets[0];
    socket.send(JSON.stringify({ type: 'control', state: 'observing' }));
    socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch-observer', offset: 0, reset: true, replay_bytes: 4,
      restore_cols: 120, restore_rows: 40, restore: '\x1b[2J\x1b[Hobserver $ ' }));
    socket.send(Buffer.from('tail'));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('observer $ tail'));
    await page.waitForFunction(() => document.querySelector('.mobile-terminal-panel:not([hidden]) .mobile-composer textarea')?.disabled === true);
    assert.equal(await page.getByRole('button', { name: '接管终端', exact: true }).count(), 1, 'observer mode exposes an explicit takeover action');
    assert.deepEqual(controlMessages(state, 'resize'), [], 'observer mode never resizes the PTY');
    assert.equal(state.messages.filter(({ message }) => typeof message !== 'string').length, 0, 'observer mode cannot write input');

    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    assert.equal(controlMessages(state, 'claim').at(-1).force, true, 'takeover button force-claims terminal control');
    await context.close();
  });

  await test('a superseded controller reconnects as an observer without retaking input', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const state = await fixture(context, undefined, 40, true);
    const page = await context.newPage();
    await page.goto(baseURL + '/m#workspace=' + encodeURIComponent(workspace) + '&view=terminal');
    await visible(page, '.mobile-terminal-tabs');
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    const controller = state.sockets[0];
    controller.send(JSON.stringify({ type: 'control', state: 'claimed' }));
    controller.send(JSON.stringify({ type: 'stream', epoch: 'epoch-controller', offset: 0, reset: true, replay_bytes: 0,
      restore_cols: 120, restore_rows: 40, restore: '\x1b[2J\x1b[Hcontroller $ ' }));
    await terminalReady(page);

    supersede(state, controller);
    await page.waitForFunction(() => document.querySelector('.mobile-composer textarea')?.disabled === true);
    for (let attempt = 0; state.sockets.length < 2 && attempt < 30; attempt += 1) await page.waitForTimeout(50);
    assert.equal(state.sockets.length, 2, 'the superseded socket reconnects automatically');
    assert.match(state.sockets[1].url(), /observer=v1/, 'replacement attach negotiates observer support');
    assert.equal(controlMessages(state, 'claim').at(-1).force, false, 'replacement attach never force-claims control');

    const observer = state.sockets[1];
    observer.send(JSON.stringify({ type: 'control', state: 'observing' }));
    observer.send(JSON.stringify({ type: 'stream', epoch: 'epoch-observer-after-takeover', offset: 0, reset: true, replay_bytes: 4,
      restore_cols: 120, restore_rows: 40, restore: '\x1b[2J\x1b[Hobserver $ ' }));
    observer.send(Buffer.from('tail'));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('observer $ tail'));
    await page.waitForFunction(() => document.querySelector('.mobile-composer textarea')?.disabled === true);
    assert.equal(await page.getByRole('button', { name: '接管终端', exact: true }).count(), 1, 'observer mode exposes takeover only after the read-only stream is ready');

    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    assert.equal(controlMessages(state, 'claim').at(-1).force, true, 'only an explicit user action force-claims control');
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('iOS keyboard uses visual height, removes safe-area gaps and preserves terminal geometry', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    await context.addInitScript(() => {
      Object.defineProperty(window, 'visualViewport', { value: Object.assign(new EventTarget(), { height: 844, width: 390, offsetTop: 0 }) });
    });
    const state = await fixture(context, undefined, 140, true);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await visible(page, '.mobile-terminal-tabs');
    await page.setViewportSize({ width: 390, height: 760 });
    await page.waitForTimeout(200);
    assert.equal(state.messages.length, 0, 'layout changes before takeover cannot claim or resize');
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await page.waitForTimeout(200);
    await page.setViewportSize({ width: 390, height: 844 });
    const socket = state.sockets[0];
    socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
    socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch-one', offset: 0, reset: true, replay_bytes: 1,
      restore_cols: 120, restore_rows: 140, restore: '\x1b[2J\x1b[HAow mobile terminal\r\n$ ' }));
    await page.waitForTimeout(300);
    assert.equal(await page.getByRole('textbox', { name: '终端命令' }).isDisabled(), true);
    assert.deepEqual(controlMessages(state, 'resize'), [], 'receiving control does not resize before replay finishes');
    socket.send(Buffer.from(' '));
    await terminalReady(page);
    const portrait = await terminalSize(page, state, (size) => size.cols < 60 && size.rows < 140);
    await terminalFits(page);
    await page.evaluate(() => document.documentElement.style.setProperty('--mobile-safe-bottom', '34px'));
    await page.waitForTimeout(250);
    const original = await terminalSize(page, state);
    const resizeCount = controlMessages(state, 'resize').length;
    const stage = await page.locator('.mobile-terminal-stage').boundingBox();
    const navigationHeight = (await page.locator('.mobile-bottom-nav').boundingBox()).height;
    await page.getByRole('textbox', { name: '终端命令' }).focus();
    // Simulate WKWebView reporting an innerHeight smaller than its visual viewport.
    await page.evaluate(() => {
      window.__innerHeightDescriptor = Object.getOwnPropertyDescriptor(window, 'innerHeight');
      Object.defineProperty(window, 'innerHeight', { configurable: true, value: 350 });
      Object.assign(window.visualViewport, { height: 420, offsetTop: 60 });
      window.visualViewport.dispatchEvent(new Event('resize'));
    });
    await page.waitForTimeout(300);
    const app = await page.locator('.mobile-app').boundingBox();
    const composer = await page.locator('.mobile-composer').boundingBox();
    assert.equal(app.y, 60);
    assert.equal(app.height, 420);
    assert.ok(composer.y + composer.height <= 480, 'input follows the visible keyboard viewport including its offset');
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), false, 'keyboard hides navigation including its home-indicator inset');
    const inputBox = await page.locator('.mobile-terminal-input').boundingBox();
    assert.equal(inputBox.y + inputBox.height, app.y + app.height, 'no navigation or safe-area gap remains below terminal input');
    assert.equal((await page.locator('.mobile-terminal-stage').boundingBox()).height, stage.height);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'keyboard visibility and safe-area changes never resize the PTY');
    const viewport = await page.locator('.mobile-terminal-viewport').boundingBox();
    const shellPrompt = await page.locator('.xterm-rows > div').filter({ hasText: '$' }).first().boundingBox();
    assert.ok(shellPrompt.y >= viewport.y && shellPrompt.y + shellPrompt.height <= viewport.y + viewport.height, 'a shell cursor near the top remains visible');
    socket.send(Buffer.from(`\x1b[?1049h\x1b[2J\x1b[${original.rows};1HCLI-PROMPT`));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('CLI-PROMPT'));
    await page.waitForTimeout(100);
    const tuiPrompt = await page.locator('.xterm-rows > div').filter({ hasText: 'CLI-PROMPT' }).boundingBox();
    assert.ok(tuiPrompt.y >= viewport.y && tuiPrompt.y + tuiPrompt.height <= viewport.y + viewport.height + 1, `a TUI prompt on the last row is visible without reflow: ${JSON.stringify({tuiPrompt, viewport, stage: await page.locator('.mobile-terminal-stage').boundingBox(), screen: await page.locator('.xterm-screen').boundingBox(), original})}`);
    await snapshot(page, 'mobile-terminal-tall-keyboard');
    // Android's system Back can hide the keyboard without blurring the input.
    await page.evaluate(() => { Object.assign(window.visualViewport, { height: 844, offsetTop: 0 }); window.visualViewport.dispatchEvent(new Event('resize')); });
    await page.waitForTimeout(200);
    assert.equal((await page.locator('.mobile-bottom-nav').boundingBox()).height, navigationHeight, 'navigation and safe area return when the keyboard closes while the input remains focused');
    assert.equal(controlMessages(state, 'resize').length, resizeCount);
    await page.evaluate(() => { Object.assign(window.visualViewport, { height: 420, offsetTop: 60 }); window.visualViewport.dispatchEvent(new Event('resize')); });
    await page.waitForTimeout(150);
    await page.getByRole('button', { name: '收起键盘' }).click();
    await page.evaluate(() => {
      Object.defineProperty(window, 'innerHeight', window.__innerHeightDescriptor);
      Object.assign(window.visualViewport, { height: 600, offsetTop: 20 });
      window.visualViewport.dispatchEvent(new Event('resize'));
    });
    await page.waitForTimeout(200);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'closing animation keeps the terminal grid stable');
    assert.equal(await page.locator('.mobile-bottom-nav').isVisible(), false, 'navigation stays hidden through intermediate closing frames');
    await page.evaluate(() => { Object.assign(window.visualViewport, { height: 844, offsetTop: 0 }); window.visualViewport.dispatchEvent(new Event('resize')); });
    await page.waitForTimeout(250);
    assert.equal((await page.locator('.mobile-app').boundingBox()).height, 844);
    assert.equal(controlMessages(state, 'resize').length, resizeCount, 'restoring the full viewport and safe area does not resize');
    assert.equal((await page.locator('.mobile-bottom-nav').boundingBox()).height, navigationHeight, 'navigation and home-indicator safe area return after dismissal');
    await page.setViewportSize({ width: 844, height: 390 });
    await page.waitForTimeout(100);
    assert.equal((await page.locator('.mobile-app').boundingBox()).height, 390, 'stale WebView visual metrics cannot overflow a shorter layout viewport');
    await terminalFits(page);
    await terminalSize(page, state, (size) => size.cols > portrait.cols && size.rows < portrait.rows);
    assert.ok(controlMessages(state, 'resize').every((size) => size.rows !== 140), 'snapshot dimensions are never echoed to the PTY');
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('keyboard animation moves a fixed terminal surface before each paint', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true, deviceScaleFactor: 3 });
    await context.addInitScript(() => {
      Object.defineProperty(window, 'visualViewport', { value: Object.assign(new EventTarget(), { height: 844, width: 390, offsetTop: 0 }) });
    });
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m#workspace=${encodeURIComponent(workspace)}&view=terminal`);
    await page.evaluate(() => document.documentElement.style.setProperty('--mobile-safe-bottom', '34px'));
    await page.getByRole('button', { name: '接管终端', exact: true }).click();
    await terminalReady(page);
    const size = await terminalSize(page, state);
    state.sockets[0].send(Buffer.from(`\x1b[?1049h\x1b[2J\x1b[${size.rows};1HMOTION-PROMPT`));
    await page.waitForFunction(() => document.querySelector('.xterm-rows')?.textContent.includes('MOTION-PROMPT'));
    await page.waitForTimeout(200);
    const resizeCount = controlMessages(state, 'resize').length;
    await page.evaluate(() => {
      const viewport = document.querySelector('.mobile-terminal-viewport');
      const stage = document.querySelector('.mobile-terminal-stage');
      const screen = document.querySelector('.xterm-screen');
      const frames = [];
      const height = stage.getBoundingClientRect().height;
      // Runs after the production ResizeObserver, before the browser paints.
      // Endpoint-only checks miss a stale translation during keyboard animation.
      const observer = new ResizeObserver(() => {
        const clip = viewport.getBoundingClientRect();
        // Removing the home-indicator inset can briefly make the viewport
        // taller than the fixed surface at the very start of keyboard opening.
        const expectedBottom = Math.min(clip.bottom, clip.top + height);
        frames.push({ height: stage.getBoundingClientRect().height,
          bottomGap: screen.getBoundingClientRect().bottom - expectedBottom });
      });
      observer.observe(viewport);
      window.__terminalMotion = { frames, height, observer };
    });
    const animate = (heights) => page.evaluate(async (heights) => {
      for (const height of heights) {
        Object.assign(window.visualViewport, { height, offsetTop: (844 - height) / 8 });
        window.visualViewport.dispatchEvent(new Event('resize'));
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
      }
    }, heights);
    for (const source of ['.mobile-composer textarea', '.xterm-helper-textarea']) {
      await page.locator(source).focus();
      await animate([811.25, 750.5, 670.75, 580.25, 495.5, 420]);
      await page.getByRole('button', { name: '收起键盘', exact: true }).tap();
      await animate([460.5, 540.25, 650.75, 770.5, 838.25, 844]);
    }
    const motion = await page.evaluate(() => {
      window.__terminalMotion.observer.disconnect();
      return { frames: window.__terminalMotion.frames, height: window.__terminalMotion.height };
    });
    assert.ok(motion.frames.length >= 20, 'both input sources exercise the intermediate keyboard frames');
    assert.ok(motion.frames.every((frame) => Math.abs(frame.height - motion.height) < .01), `surface dimensions stay fixed: ${JSON.stringify(motion)}`);
    assert.ok(motion.frames.every((frame) => Math.abs(frame.bottomGap) <= 1), `terminal position is updated before painting: ${JSON.stringify(motion.frames)}`);
    assert.equal(controlMessages(state, 'resize').length, resizeCount);
    assert.equal(state.sockets.length, 1);
    assert.deepEqual(state.errors, []);
    await context.close();
  });

  await test('pins migrate once into server storage and sync across independent browsers', async t => {
    const legacy = ['/workspace/mobile', workspace, '/removed-worktree'];
    const pins = { paths: ['/server-only'], revision: 1, failWrites: true };
    const desktopContext = await browser.newContext({ viewport: { width: 1440, height: 900 },
      storageState: { cookies: [], origins: [{ origin: new URL(baseURL).origin, localStorage: [{ name: 'aow-pinned-worktrees', value: JSON.stringify(legacy) }] }] } });
    t.after(() => desktopContext.close());
    const desktopState = await fixture(desktopContext, pins);
    const desktop = await desktopContext.newPage();
    await desktop.goto(`${baseURL}/?ui=desktop`);
    await desktop.getByRole('alert').filter({ hasText: '置顶同步失败' }).waitFor();
    assert.deepEqual(await desktop.evaluate(() => JSON.parse(localStorage.getItem('aow-pinned-worktrees'))), legacy, 'failed migration keeps the original browser records');
    pins.failWrites = false;
    await desktop.evaluate(() => window.dispatchEvent(new Event('focus')));
    await desktop.waitForFunction(() => localStorage.getItem('aow-pinned-worktrees-migrated') === '1');
    await desktop.waitForFunction(() => document.querySelectorAll('.project-aow-pinned > button').length === 2);
    assert.deepEqual(pins.paths, ['/server-only', ...legacy], 'migration merges existing server pins and deduplicates paths');
    assert.equal(await desktop.evaluate(() => localStorage.getItem('aow-pinned-worktrees')), null);
    assert.equal(await desktop.evaluate(() => localStorage.getItem('aow-pinned-worktrees-migrated')), '1');

    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    t.after(() => context.close());
    const state = await fixture(context, pins);
    const page = await context.newPage();
    await page.goto(`${baseURL}/m`);
    const pinned = page.getByRole('region', { name: 'Pinned', exact: true });
    await visible(page, '.mobile-pinned .mobile-worktree-row');
    assert.deepEqual(await pinned.locator('.mobile-worktree-row strong').allTextContents(), ['feat/mobile-experience', 'main']);
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-pinned-worktrees')), null, 'phone sees desktop pins with independent browser storage');
    await pinned.getByRole('button', { name: /main.*主目录/ }).click();
    await visible(page, '.mobile-terminal-tabs');
    assert.equal(state.sockets.length, 0, 'opening a pinned workspace does not claim a terminal');
    await page.getByRole('button', { name: '返回项目列表' }).click();
    await visible(page, '.mobile-pinned .mobile-worktree-row');
    const search = page.getByRole('searchbox', { name: '搜索项目或分支' });
    await search.fill('feat/mobile');
    assert.deepEqual(await pinned.locator('.mobile-worktree-row strong').allTextContents(), ['feat/mobile-experience']);
    await search.fill('');
    await snapshot(page, 'mobile-projects-pinned');
    await pinned.getByRole('button', { name: '取消置顶 AOW · feat/mobile-experience', exact: true }).click();
    await page.waitForFunction(() => document.querySelectorAll('.mobile-pinned .mobile-worktree-row').length === 1);
    assert.deepEqual(pins.paths, ['/server-only', workspace, '/removed-worktree']);
    await desktop.evaluate(() => window.dispatchEvent(new Event('focus')));
    await desktop.waitForFunction(() => document.querySelectorAll('.project-aow-pinned > button').length === 1);

    pins.failWrites = true;
    await page.getByRole('button', { name: '置顶 AOW · feat/mobile-experience', exact: true }).click();
    await page.getByRole('alert').filter({ hasText: '置顶同步失败' }).waitFor();
    assert.deepEqual(await pinned.locator('.mobile-worktree-row strong').allTextContents(), ['main'], 'failed config writes do not pretend pins were saved');
    pins.failWrites = false;
    await page.getByRole('button', { name: '置顶 AOW · feat/mobile-experience', exact: true }).click();
    await page.waitForFunction(() => document.querySelectorAll('.mobile-pinned .mobile-worktree-row').length === 2);
    await page.reload();
    await visible(page, '.mobile-pinned .mobile-worktree-row');
    assert.deepEqual(await pinned.locator('.mobile-worktree-row strong').allTextContents(), ['main', 'feat/mobile-experience']);
    await desktop.evaluate(() => window.dispatchEvent(new Event('focus')));
    await desktop.waitForFunction(() => document.querySelectorAll('.project-aow-pinned > button').length === 2);
    await desktop.locator('.project-aow-pinned > button').filter({ hasText: 'feat/mobile-experience' }).click({ button: 'right' });
    await desktop.getByRole('menuitem', { name: 'Unpin', exact: true }).click();
    await desktop.waitForFunction(() => document.querySelectorAll('.project-aow-pinned > button').length === 1);
    await page.getByRole('button', { name: '刷新', exact: true }).click();
    await page.waitForFunction(() => document.querySelectorAll('.mobile-pinned .mobile-worktree-row').length === 1);
    await desktop.evaluate((paths) => localStorage.setItem('aow-pinned-worktrees', JSON.stringify(paths)), legacy);
    await desktop.reload();
    await desktop.waitForFunction(() => document.querySelectorAll('.project-aow-pinned > button').length === 1);
    assert.deepEqual(pins.paths, ['/server-only', workspace, '/removed-worktree'], 'legacy records cannot restore a pin that was later removed');
    await noOverflow(page);
    assert.deepEqual(state.mutations, []);
    assert.deepEqual(state.errors, []);
    assert.deepEqual(desktopState.errors, []);
  });

  await test('mobile conversation renders Mermaid in production after navigation and refresh', async () => {
    const context = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
    const state = await fixture(context);
    const diagrams = [
      '```mermaid', 'flowchart LR', '  A[用户请求] --> B[处理任务]', '  B --> C[完成]', '```', '',
      '```mermaid', 'sequenceDiagram', '  participant U as 用户', '  participant S as 服务', '  U->>S: 读取会话', '  S-->>U: 返回结果', '```',
    ].join('\n');
    await context.route('**/api/aow/agent-sessions/session-one/snapshot?**', route => route.fulfill({ json: {
      ...session, captured_at: session.updated_at, status: 'completed', truncated: false,
      turns: [{ id: 'diagram-turn', status: 'completed', user: { text: '查看流程图和时序图', timestamp: session.created_at },
        final: { text: diagrams, timestamp: session.updated_at } }],
    } }));
    try {
      const page = await context.newPage();
      await page.goto(`${baseURL}/m`);
      await page.getByRole('button', { name: /main.*主目录/ }).click();
      await page.getByRole('button', { name: 'Conversation', exact: true }).click();
      await page.getByRole('button', { name: /实现手机端工作台/ }).click();
      const rendered = page.locator('.mobile-conversation .project-aow-snapshot-flowchart > svg');
      async function expectDiagrams() {
        await rendered.nth(1).waitFor();
        assert.equal(await rendered.count(), 2);
        for (const diagram of await rendered.all()) {
          const bounds = await diagram.boundingBox();
          assert.ok(bounds.width > 100 && bounds.height > 35, 'the diagram must be visible at a readable size');
        }
        assert.equal(await page.locator('.project-aow-snapshot-flowchart-error').count(), 0);
        await noOverflow(page);
      }
      await expectDiagrams();
      const refreshed = page.waitForResponse(response => new URL(response.url()).pathname.endsWith('/session-one/snapshot'));
      await page.getByRole('button', { name: '刷新', exact: true }).click();
      await (await refreshed).finished();
      await page.locator('button[aria-label="刷新"]:enabled').waitFor();
      await expectDiagrams();
      await page.reload();
      await expectDiagrams();
      await snapshot(page, 'mobile-conversation-mermaid');
      assert.deepEqual(state.errors, []);
    } finally {
      await context.close();
    }
  });

  await test('desktop defers Monaco until a task editor is opened', async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const state = await fixture(context);
    try {
      const page = await context.newPage();
      await page.goto(`${baseURL}/aow/?ui=desktop`);
      await page.locator('.project-aow-worktrees > button').first().click();
      await visible(page, '.terminal-emulator-shell:not(.restore-pending)');
      const editorLoaded = () => page.evaluate(() => typeof window.MonacoEnvironment !== 'undefined');
      assert.equal(await editorLoaded(), false, 'a terminal-only workspace must not initialize Monaco');
      await page.getByRole('button', { name: 'Automation', exact: true }).click();
      await page.locator('.automation-panel-row').first().waitFor();
      assert.equal(await editorLoaded(), false, 'listing tasks must not load their editor');
      await page.getByRole('button', { name: '创建自动化', exact: true }).click();
      const dialog = page.getByRole('dialog', { name: '创建自动化', exact: true });
      await dialog.locator('.monaco-editor').waitFor();
      assert.equal(await editorLoaded(), true, 'opening the editor initializes its local Monaco runtime');
      await dialog.getByRole('button', { name: '关闭', exact: true }).click();
      await dialog.waitFor({ state: 'hidden' });
      assert.deepEqual(state.mutations, []);
      assert.deepEqual(state.errors, []);
    } finally {
      await context.close();
    }
  });

  await test('PR detail fills the workspace and preserves inline diff and tab interactions', async () => {
    const context = await browser.newContext({ viewport: { width: 2200, height: 1000 } });
    const state = await fixture(context);
    const pr = {
      number: 3099, status: 'open', draft: false, title: 'Full-width PR layout',
      source_branch: 'feature/pr-layout', target_branch: 'main', url: null,
      created_at: '2026-09-12T08:00:00Z', updated_at: '2026-09-12T09:00:00Z',
      description: '# Description\n\n' + 'Long Markdown description with `inline code`. '.repeat(50)
        + '\n\n```json\n' + JSON.stringify({ output: 'captured output '.repeat(100) }) + '\n```',
      changes_count: 2, commits_count: 1, review_status: 'passed', check_summary_status: 'passed', mergeable: true,
      reviewers: [], checks: [{ name: 'Build', status: 'completed', conclusion: 'passed', details_url: null }],
      unresolved_threads: [{ id: 'thread-1', path: null, line: null, status: 'open', author: 'Reviewer', body: 'Review feedback. '.repeat(80), updated_at: null }],
      files: [{ path: 'frontend/src/main.tsx', change_type: 'modified' }, { path: 'frontend/src/api.ts', change_type: 'modified' }],
    };
    await context.route('**/api/review-targets?**', route => route.fulfill({ json: [] }));
    await context.route('**/api/my-pull-requests**', async (route) => {
      assert.equal(route.request().method(), 'GET');
      const url = new URL(route.request().url());
      const data = url.pathname.endsWith('/diff')
        ? { repository: workspace, number: pr.number, path: url.searchParams.get('path'), original_path: null,
          original: 'const value = "before";\n', modified: 'const value = "after";\n', binary: false, truncated: false }
        : url.pathname.endsWith(`/${pr.number}`) ? pr
          : { repository: workspace, current_branch: 'main', current_user: { id: 'user', username: 'me', display_name: 'Me' }, pull_requests: [pr] };
      await route.fulfill({ json: data });
    });
    await context.route('**/api/git/diff?**', (route) => route.fulfill({ json: {
      repository: workspace, path: 'frontend/src/main.tsx', staged: false, patch: '', truncated: false,
      original: 'const value = "before";\n', modified: 'const value = "after";\n',
    } }));
    try {
      const page = await context.newPage();
      await page.goto(baseURL + '/?ui=desktop');
      const projectButton = page.getByRole('button', { name: /^(展开|收起) AOW$/ });
      if (await projectButton.getAttribute('aria-expanded') === 'false') await projectButton.click();
      await page.locator('.project-aow-worktrees > button').first().click();
      await page.getByRole('button', { name: 'Pull Requests', exact: true }).click();
      await page.locator('.my-pr-row').click();
      const detail = page.getByRole('article', { name: `PR #${pr.number} 详情`, exact: true });
      await detail.waitFor();
      const body = detail.locator('.pr-scroll');
      async function fillsMain(selector) {
        const bodyWidth = await detail.locator('.pr-main').evaluate((node) => node.clientWidth);
        const childWidth = await detail.locator(selector).evaluate((node) => node.getBoundingClientRect().width);
        assert.ok(Math.abs(childWidth - bodyWidth) <= 1, `${selector} should fill ${bodyWidth}px, got ${childWidth}px`);
        assert.equal(await body.evaluate((node) => node.scrollWidth <= node.clientWidth), true, 'PR body must not overflow horizontally');
      }
      assert.ok(await body.evaluate((node) => node.clientWidth) > 1100, 'wide viewport exposes the old content width limit');
      const hostBounds = await page.locator('.project-aow-pull-request-host:visible').boundingBox();
      assert.deepEqual(await detail.boundingBox(), hostBounds, 'PR occupies the entire tab host');
      await fillsMain('.pr-description');
      await fillsMain('[role="tabpanel"]:not([hidden])');
      await snapshot(page, 'pr-wide-description');
      await detail.getByRole('button', { name: '描述', exact: true }).click();
      assert.equal(await detail.locator('.pr-description .pr-card-body').isVisible(), false);
      await detail.getByRole('tab', { name: /文件变更/ }).click();
      const fileButtons = detail.locator('.pr-file-header');
      await fileButtons.nth(0).click();
      await visible(page, '.pr-inline-diff .monaco-diff-editor');
      await fileButtons.nth(1).click();
      await page.waitForFunction(() => document.querySelectorAll('.pr-inline-diff .monaco-diff-editor').length === 2);
      await fillsMain('.pr-files');
      const diff = detail.locator('.monaco-diff-editor').first();
      const original = await diff.locator('.editor.original').boundingBox();
      const modified = await diff.locator('.editor.modified').boundingBox();
      assert.ok(original.width > 0 && modified.x >= original.x + original.width - 1, 'diff remains side by side');
      await fileButtons.nth(0).click();
      assert.equal(await detail.locator('.pr-inline-diff').nth(0).isVisible(), false);
      await detail.getByRole('tab', { name: /检查/ }).click();
      await fillsMain('[role="tabpanel"]:not([hidden])');
      await detail.getByRole('tab', { name: /讨论/ }).click();
      await fillsMain('[role="tabpanel"]:not([hidden])');
      await snapshot(page, 'pr-wide-feedback');
      await page.setViewportSize({ width: 1280, height: 800 });
      await fillsMain('[role="tabpanel"]:not([hidden])');
      await noOverflow(page);
      await detail.getByRole('tab', { name: /文件变更/ }).click();
      assert.equal(await fileButtons.nth(1).getAttribute('aria-expanded'), 'true', 'switching tabs preserves expanded files');
      await page.getByRole('button', { name: 'Source Control', exact: true }).click();
      await page.locator('.change-row:visible').first().click();
      await visible(page, '.project-aow-document-host .monaco-diff-editor');
      assert.deepEqual(state.mutations, [], 'PR layout interactions remain read-only');
      assert.deepEqual(state.errors, []);
    } finally {
      await context.close();
    }
  });

  await test('desktop terminal links use a dismissible workspace confirmation', async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const state = await fixture(context);
    const nativeDialogs = [];
    const destination = 'https://example.com/terminal-link?from=aow';
    await context.route('https://example.com/**', (route) => route.fulfill({ contentType: 'text/html', body: '<h1>Link destination</h1>' }));
    try {
      const page = await context.newPage();
      page.on('dialog', async (dialog) => { nativeDialogs.push(dialog.message()); await dialog.dismiss(); });
      await page.goto(baseURL + '/?ui=desktop');
      const projectButton = page.getByRole('button', { name: /^(展开|收起) AOW$/ });
      if (await projectButton.getAttribute('aria-expanded') === 'false') await projectButton.click();
      await page.locator('.project-aow-worktrees > button').first().click();
      const shell = page.locator('.terminal-pane .terminal-emulator-shell').first();
      await shell.locator('.terminal-connection.connected').waitFor({ state: 'attached' });
      await shell.locator('.terminal-restore-status').waitFor({ state: 'detached' });
      const socket = state.sockets.find((socket) => /panes\/shell\/ws/.test(socket.url()));
      assert.ok(socket);
      const dialog = page.getByRole('alertdialog', { name: '打开链接', exact: true });
      async function clickLink(uri = destination) {
        await page.mouse.move(0, 0);
        socket.send(Buffer.from(`\x1b[2J\x1b[H\x1b]8;;${uri}\x07OPEN-LINK\x1b]8;;\x07 suffix`));
        await page.waitForTimeout(150);
        const bounds = await shell.locator('.xterm-screen').boundingBox();
        // Move across another row so xterm refreshes its cached link provider result.
        await page.mouse.move(bounds.x + 20, bounds.y + 30);
        await page.mouse.click(bounds.x + 20, bounds.y + 8);
      }
      async function closed() {
        await dialog.waitFor({ state: 'detached' });
        assert.equal(await page.locator(':modal').count(), 0, 'dismissal releases the modal backdrop');
        await shell.locator('.xterm-helper-textarea').focus();
        assert.equal(await shell.locator('.xterm-helper-textarea').evaluate((node) => node === document.activeElement), true, 'terminal can receive focus again');
        assert.equal(context.pages().length, 1, 'cancel never opens a destination');
      }
      for (const uri of ['', '   ', 'invalid-url', 'javascript:alert(1)', 'file:///tmp/test']) {
        await clickLink(uri);
        assert.equal(await dialog.count(), 0, `ignored URL: ${uri}`);
      }
      await clickLink();
      await dialog.waitFor();
      assert.equal(await dialog.locator('.confirmation-items span').textContent(), destination);
      await snapshot(page, 'desktop-terminal-link-confirmation');
      assert.equal(await dialog.getByRole('button', { name: '取消', exact: true }).evaluate((node) => node === document.activeElement), true);
      await page.keyboard.press('Shift+Tab');
      await page.keyboard.press('Shift+Tab');
      assert.equal(await dialog.getByRole('button', { name: '打开链接', exact: true }).evaluate((node) => node === document.activeElement), true, 'Tab stays inside the dialog');
      await dialog.getByRole('button', { name: '取消', exact: true }).click();
      await closed();
      for (const dismiss of [
        () => page.keyboard.press('Escape'),
        () => dialog.getByRole('button', { name: '关闭对话框', exact: true }).click(),
        () => page.mouse.click(4, 4),
      ]) {
        await clickLink();
        await dialog.waitFor();
        await dismiss();
        await closed();
      }
      const longUrl = 'https://example.com/' + 'long-path-'.repeat(500);
      await clickLink(longUrl);
      await dialog.waitFor();
      await page.setViewportSize({ width: 390, height: 844 });
      const bounds = await dialog.boundingBox();
      const viewport = page.viewportSize();
      assert.ok(bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= viewport.width && bounds.y + bounds.height <= viewport.height);
      assert.equal(await dialog.locator('.confirmation-items span').textContent(), longUrl);
      await snapshot(page, 'narrow-terminal-link-confirmation');
      await dialog.getByRole('button', { name: '取消', exact: true }).click();
      await page.setViewportSize({ width: 1440, height: 900 });
      await closed();
      await clickLink();
      await dialog.waitFor();
      const popupReady = page.waitForEvent('popup');
      await dialog.getByRole('button', { name: '打开链接', exact: true }).click();
      const popup = await popupReady;
      await popup.waitForURL(destination);
      assert.equal(await popup.evaluate(() => window.opener), null);
      await popup.close();
      await closed();
      await clickLink();
      await dialog.waitFor();
      // Simulate a workspace change while confirmation is pending, bypassing
      // the modal's normal input lock as a server-driven update could do.
      await page.locator('.project-aow-center-tab[title="服务"]').evaluate((node) => node.click());
      await dialog.waitFor({ state: 'detached' });
      assert.equal(await page.locator(':modal').count(), 0);
      await page.locator('.project-aow-center-tab[title="开发"]').click();
      await closed();
      await clickLink();
      await dialog.waitFor();
      await dialog.getByRole('button', { name: '取消', exact: true }).click();
      await closed();
      assert.deepEqual(nativeDialogs, [], 'terminal links never call native confirm');
      assert.deepEqual(state.errors, []);
      assert.deepEqual(state.mutations, []);
    } finally {
      await context.close();
    }
  });

  await test('desktop opens files by absolute, home and current-worktree-relative paths', async () => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ['clipboard-read', 'clipboard-write'] });
    const state = await fixture(context);
    let homeUnavailable = false;
    await context.route('**/api/fs/home', route => route.fulfill(homeUnavailable
      ? { status: 403, json: { message: 'Home directory unavailable' } }
      : { json: { path: '/home/test-user', entries: [] } }));
    try {
      const page = await context.newPage();
      const showDialog = async () => {
        await page.getByRole('button', { name: '新建窗体', exact: true }).click();
        await page.getByRole('button', { name: '打开文件', exact: true }).click();
        return page.getByRole('dialog', { name: '打开文件' });
      };
      const openPath = async (input, path, requestPath = path) => {
        const dialog = await showDialog();
        await dialog.getByRole('textbox', { name: '文件路径', exact: true }).fill(input);
        const request = page.waitForRequest(request => decodeURIComponent(new URL(request.url()).pathname) === `/api/fs/raw${requestPath}`);
        await dialog.getByRole('button', { name: '打开文件', exact: true }).click();
        await request;
        await dialog.waitFor({ state: 'detached' });
        await page.locator('.editor-toolbar:visible code').filter({ hasText: path }).waitFor();
        assert.equal(await page.getByRole('button', { name: /立即保存|已自动保存/ }).count(), 1, 'opened files support saving');
      };
      await page.goto(baseURL + '/?ui=desktop');
      await visible(page, '.project-aow');
      const projectButton = page.getByRole('button', { name: /^(展开|收起) AOW$/ });
      if (await projectButton.getAttribute('aria-expanded') === 'false') await projectButton.click();
      await page.locator('.project-aow-worktrees > button').first().click();
      await openPath('/tmp/external.md', '/tmp/external.md');
      await page.locator('.project-aow-center-tab[title="external.md"]').click({ button: 'right' });
      assert.equal(await page.getByRole('menuitem', { name: '重命名', exact: true }).count(), 0, 'external files cannot be renamed from the aow');
      await page.keyboard.press('Escape');

      await openPath('  relative.txt  ', `${workspace}/relative.txt`);
      await openPath('./src/README.md', `${workspace}/./src/README.md`, `${workspace}/src/README.md`);
      await openPath('src/路径 with spaces #1%.txt', `${workspace}/src/路径 with spaces #1%.txt`);

      await page.locator('.project-aow-worktrees > button').nth(1).click();
      const feature = project.worktrees[1].path;
      await page.getByRole('button', { name: 'Explorer', exact: true }).click();
      await page.locator(`.tree-row[data-tree-path="${feature}/src"]`).click();
      await page.locator(`.tree-row[data-tree-path="${feature}/src/README.md"]`).click({ button: 'right' });
      await page.getByRole('menuitem', { name: '复制 • 相对路径', exact: true }).click();
      const copied = await page.evaluate(() => navigator.clipboard.readText());
      assert.equal(copied, 'src/README.md');
      await openPath(copied, `${feature}/src/README.md`);
      await openPath('~/home-file.md', '/home/test-user/home-file.md');
      await openPath('/tmp/absolute-from-feature.md', '/tmp/absolute-from-feature.md');

      const invalid = await showDialog();
      await invalid.getByRole('textbox', { name: '文件路径', exact: true }).fill('   ');
      assert.equal(await invalid.getByRole('button', { name: '打开文件', exact: true }).isDisabled(), true);
      homeUnavailable = true;
      await invalid.getByRole('textbox', { name: '文件路径', exact: true }).fill('~/unavailable.md');
      await invalid.getByRole('button', { name: '打开文件', exact: true }).click();
      await invalid.getByRole('alert').filter({ hasText: 'Home directory unavailable' }).waitFor();
      await invalid.getByRole('textbox', { name: '文件路径', exact: true }).fill('/tmp/not-a-file');
      await invalid.getByRole('button', { name: '打开文件', exact: true }).click();
      await invalid.getByRole('alert').filter({ hasText: 'not a regular file' }).waitFor();
      await invalid.getByRole('button', { name: '取消', exact: true }).click();
      assert.deepEqual(state.errors, []);
      assert.deepEqual(state.mutations, []);
    } finally {
      await context.close();
    }
  });

  await test('desktop keeps split layout and resizes its terminal normally', async t => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    t.after(() => context.close());
    const state = await fixture(context);
    const page = await context.newPage();
    await page.goto(baseURL + '/?ui=desktop');
    await visible(page, '.project-aow');
    const projectButton = page.getByRole('button', { name: /^(展开|收起) AOW$/ });
    if (await projectButton.getAttribute('aria-expanded') === 'false') await projectButton.click();
    await page.locator('.project-aow-worktrees > button').first().click();
    await visible(page, '.terminal-split');
    await page.waitForTimeout(500);
    assert.ok(state.messages.some(entry => typeof entry.message === 'string' && JSON.parse(entry.message).type === 'resize'), 'desktop must still fit terminal to its panes');
    assert.equal(await page.locator('.mobile-app').count(), 0);
    await snapshot(page, 'desktop-preserved');
    assert.deepEqual(state.errors, []);
  });
} finally {
  await browser?.close();
  if (server) await new Promise((resolve) => server.httpServer.close(resolve));
}
