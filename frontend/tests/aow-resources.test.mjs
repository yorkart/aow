import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
const worktrees = [0, 1].map(i => ({ id: `wt-${i}`, project_id: 'project', path: `/workspace/wt-${i}`, branch: `branch-${i}`, head: 'abc', is_main: i === 0, detached: false, locked: false, prunable: false }));
const project = { id: 'project', name: 'Resource Fixture', registered_path: worktrees[0].path, common_git_dir: '/workspace/.git', notes_path: '/notes', worktrees };
const task = { id: 'task-one', revision: 1, name: 'Fixture Task', prompt: 'fixture', agent: 'codex', project_id: 'project', project_name: project.name, workspace_mode: 'existing', workspace_path: worktrees[0].path, cron: '0 9 * * *', interval_seconds: null, enabled: true, max_concurrent_runs: 1, yolo: false, is_running: false, last_run: null, created_at: '2026-09-15T00:00:00Z', updated_at: '2026-09-15T00:00:00Z' };
const run = { id: 'run-one', task_id: task.id, task_name: task.name, agent: 'codex', source: 'scheduled', status: 'completed', started_at: task.created_at, finished_at: task.updated_at, workspace_path: worktrees[0].path, branch: 'main', duration_ms: 1000, session_id: 'session-one', agent_command: ['codex'], exit_code: 0 };
const detailPath = '/api/aow/automations/task-one';
const runsPath = `${detailPath}/runs`;
const diffFiles = [0, 1, 2].map(index => {
  const lines = Array.from({ length: 8 + index * 3 }, (_, line) => `File ${index}: unchanged line ${line + 1}`);
  const changedLine = index + 3;
  const modified = [...lines];
  modified[changedLine - 1] = `File ${index}: updated line ${changedLine}`;
  return {
    original: `${lines.join('\n')}\n`, modified: `${modified.join('\n')}\n`,
    patch: `@@ -${changedLine} +${changedLine} @@\n-${lines[changedLine - 1]}\n+${modified[changedLine - 1]}\n`,
    changedLine,
  };
});
const surface = page => page.locator('.project-aow-surface:not([hidden])');
const switchWorktree = (page, i) => page.locator(`.project-aow-worktrees button[title="${worktrees[i].path}"]`).dispatchEvent('click');
const tab = (page, name) => surface(page).locator('.project-aow-center-tab').filter({ has: page.locator(`:scope > span`, { hasText: name }) });

async function eventually(check) {
  for (let i = 0; i < 100; i++) {
    if (await check()) return;
    await delay(50);
  }
  assert.fail('Timed out waiting for browser state');
}

async function modelPaths(page) {
  return page.evaluate(async () => {
    const { monaco } = await import('/src/features/editor/monaco.ts');
    return monaco.editor.getModels().map(model => model.uri.toString());
  });
}

let browser;
try {
  await server.listen();
  const baseURL = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });

  async function fixture(t, { fallback = false, clock = false, multipleTerminals = false, extraTerminals = 0, cliTerminals = 0, splitTerminal = false, registeredAgents = [], floating = false, beforeOpen } = {}) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    const state = { errors: [], requests: [], sessionAgents: [], projects: [project], task: { ...task }, detailGate: null, agents: {}, titles: {}, names: {}, paneNames: {}, agentFailure: false };
    state.appearanceWrites = [];
    state.pinnedWorktrees = { paths: [], revision: 0 };
    if (floating) state.projects.unshift({ ...project, id: '__aow_floating', name: '浮动工作区', builtin: true, registered_path: '/global', notes_path: '/notes/localhost/test/__aow_floating', worktrees: [{ ...worktrees[0], id: 'global-main', project_id: '__aow_floating', path: '/global', branch: 'main' }] });
    state.createdTerminals = [];
    state.terminalListScopes = [];
    state.terminalAgentScopes = [];
    state.closedTerminalIds = [];
    state.defaultTerminalRoots = worktrees.map(worktree => worktree.path);
    state.createdTerminalTabs = Array.from({ length: cliTerminals }, (_, index) => ({
      id: `cli-${index}`, name: `CLI agent ${index}`, workspace_root: worktrees[0].path, revision: 1,
      layout: { type: 'pane', pane_id: `cli-pane-${index}` }, panes: [{ id: `cli-pane-${index}`, name: 'codex', kind: 'agent', agent_id: 'codex',
        cwd: worktrees[0].path, shell: '/bin/codex', status: 'running', rows: 48, cols: 160,
        agent_terminal: { phase: 'starting', error: null, task_submitted: false } }],
    }));
    state.terminalMessages = [];
    state.terminalSockets = [];
    state.closedTerminalConnections = [];
    state.failedAgentRequests = 0;
    state.diffFiles = structuredClone(diffFiles);
    state.diffRequests = [];
    state.diffGate = null;
    state.commits = [];
    state.staged = false;
    state.textFiles = Object.fromEntries(worktrees.map(worktree => [`${worktree.path}/edit.txt`, { content: 'Initial text', version: 'v1' }]));
    state.textGate = null;
    state.failText = false;
    state.failPreview = false;
    state.previewColor = 'red';
    state.writes = [];
    state.settings = { notes_base: '/notes', execution_path: ['/usr/local/bin', '/usr/bin', '/bin'] };
    state.settingsUpdates = [];
    state.reviewProviders = { revision: 0, providers: [{ id: 'github', name: 'GitHub', enabled: true, hosts: ['github.com'], script: '# GitHub adapter\nprint("initial")\n' }] };
    state.reviewWrites = [];
    state.reviewTests = [];
    state.reviewTargets = [];

    state.registeredAgents = structuredClone(registeredAgents);
    state.agentUpdates = [];
    state.failAgentSave = false;
    state.removalJobs = [];
    state.removalSubmissions = [];
    state.dirtyWorktrees = [];
    state.otherOperations = [];
    state.operationLogs = [];
    state.logQueries = [];
    state.operationRevision = 0;
    const operationSnapshot = () => {
      const operations = [...state.otherOperations, ...state.removalJobs.map(job => {
        const active = job.items.some(item => ['queued', 'running'].includes(item.status));
        const succeeded = job.items.filter(item => item.status === 'succeeded').length;
        return { id: job.id, kind: 'worktree.remove', source: 'web', title: '删除 Worktree', project_id: job.project_id,
          resource: null, created_at: job.created_at, message: active ? '正在清理' : '清理完成',
          completed: job.items.filter(item => !['queued', 'running'].includes(item.status)).length, total: job.items.length,
          outcome: active ? null : succeeded === job.items.length ? 'succeeded' : succeeded ? 'partial_success' : 'failed' };
      })];
      const key = JSON.stringify(operations);
      if (state.lastOperations !== key) { state.lastOperations = key; state.operationRevision += 1; }
      return { boot_id: 'fixture', revision: state.operationRevision, operations, log_error: null };
    };
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); });
    await context.route('**/api/**', async route => {
      const request = route.request();
      const url = new URL(request.url());
      state.requests.push(url.pathname);
      let data;
      if (url.pathname === '/api/aow/projects/project' && request.method() === 'DELETE') {
        state.projects = [];
        await route.fulfill({ status: 204 });
        return;
      }
      if (url.pathname === '/api/auth/status') data = { configured: true, authenticated: true };
      else if (url.pathname === '/api/operations/active') data = operationSnapshot();
      else if (url.pathname === '/api/operations/stream') {
        await route.fulfill({ contentType: 'text/event-stream', body: `event: operations\ndata: ${JSON.stringify(operationSnapshot())}\n\n` });
        return;
      }
      else if (url.pathname === '/api/operation-logs') {
        const query = Object.fromEntries(url.searchParams);
        state.logQueries.push(query);
        data = state.readLogs ? state.readLogs(query) : { items: state.operationLogs.filter(item =>
          (!query.operation_id || item.operation_id === query.operation_id) && (!query.source || item.source === query.source)),
          next_cursor: null, budget_exhausted: false, scanned_bytes: 500 };
      }
      else if (url.pathname === '/api/aow/projects') data = state.projects;
      else if (/\/api\/aow\/projects\/[^/]+\/avatar$/.test(url.pathname)) {
        await state.avatarGate;
        if (state.avatarFailure) { await route.fulfill({ status: 503, json: { message: 'Avatar unavailable' } }); return; }
        data = { avatar_url: state.avatarUrl ?? null };
      }
      else if (url.pathname === '/api/aow/worktree-removals') data = state.removalJobs;
      else if (url.pathname.endsWith('/worktrees/removals') || (url.pathname.endsWith('/worktrees/removal') && request.method() === 'DELETE')) {
        const items = request.method() === 'DELETE' ? [{ path: url.searchParams.get('path'), force: url.searchParams.get('force') === 'true' }] : request.postDataJSON().items;
        state.removalSubmissions.push(items);
        const job = { id: `removal-${state.removalJobs.length}`, project_id: 'project', created_at: new Date().toISOString(), items: items.map(item => ({ ...item, status: 'queued', error: null })) };
        state.removalJobs.push(job);
        await route.fulfill({ status: 202, json: job });
        return;
      }
      else if (url.pathname.endsWith('/worktrees/removal')) {
        const path = url.searchParams.get('path');
        const worktree = state.projects.flatMap(project => project.worktrees).find(worktree => worktree.path === path);
        const dirty = state.dirtyWorktrees.includes(path);
        data = { worktree, dirty, changes: dirty ? ['?? uncommitted.txt'] : [], change_count: dirty ? 1 : 0, truncated: false, terminal_tabs: 0, agent_tabs: 0 };
      }
      else if (/\/api\/aow\/projects\/project\/worktrees\/(color|icon)$/.test(url.pathname)) {
        const update = request.postDataJSON();
        state.appearanceWrites.push(update);
        await state.appearanceGate;
        if (state.failAppearance) { await route.fulfill({ status: 500, json: { message: 'Appearance save failed' } }); return; }
        const { path, ...appearance } = update;
        state.projects = state.projects.map(item => ({ ...item, worktrees: item.worktrees.map(worktree => worktree.path === path ? { ...worktree, ...appearance } : worktree) }));
        data = state.projects.find(item => item.id === 'project');
      }
      else if (url.pathname === '/api/aow/agents') {
        if (request.method() === 'POST') {
          const update = request.postDataJSON();
          state.agentUpdates.push(update);
          if (state.failAgentSave) { await route.fulfill({ status: 500, json: { message: 'Agent configuration write failed' } }); return; }
          const id = update.id ?? 'custom-new';
          const existing = state.registeredAgents.find(agent => agent.id === id);
          data = { ...existing, ...update, id, source: 'configured', available: true, executable: `/usr/bin/${update.command}` };
          state.registeredAgents = [...state.registeredAgents.filter(agent => agent.id !== id), data];
        } else data = state.registeredAgents;
      }
      else if (url.pathname.startsWith('/api/aow/agents/') && request.method() === 'DELETE') {
        const id = decodeURIComponent(url.pathname.split('/').at(-1));
        state.registeredAgents = state.registeredAgents.filter(agent => agent.id !== id);
        const detected = registeredAgents.find(agent => agent.id === id && agent.source === 'detected');
        if (detected) state.registeredAgents.push(structuredClone(detected));
        await route.fulfill({ status: 204 });
        return;
      }
      else if (url.pathname === '/api/aow/settings') {
        if (request.method() === 'PUT') {
          const update = request.postDataJSON();
          state.settingsUpdates.push(update);
          if (state.failSettingsSave) { await route.fulfill({ status: 500, json: { message: 'Settings write failed' } }); return; }
          if (update.notes_base && update.notes_base !== state.settings.notes_base) {
            const previous = state.settings.notes_base;
            state.projects = state.projects.map(item => ({ ...item, notes_path: update.notes_base + item.notes_path.slice(previous.length) }));
            for (const [path, file] of Object.entries(state.textFiles)) if (path.startsWith(previous + '/')) {
              state.textFiles[update.notes_base + path.slice(previous.length)] = { ...file, version: 'migrated-version' };
            }
          }
          Object.assign(state.settings, update);
        }
        data = state.settings;
      }
      else if (url.pathname === '/api/aow/review-providers') {
        if (request.method() === 'PUT') {
          const update = request.postDataJSON();
          state.reviewWrites.push(update);
          if (state.failReviewSave) { await route.fulfill({ status: 409, json: { message: '配置已更新，请重新加载后保存。' } }); return; }
          state.reviewProviders = { ...update, revision: update.revision + 1 };
        }
        data = state.reviewProviders;
      }
      else if (url.pathname === '/api/aow/review-providers/test') { state.reviewTests.push(request.postDataJSON()); data = { operations: ['list', 'detail', 'diff'] }; }
      else if (url.pathname === '/api/review-targets') data = state.reviewTargets;
      else if (url.pathname === '/api/aow/settings/discovered-path') data = ['/discovered/bin', '/usr/bin', '/bin'];
      else if (url.pathname === '/api/aow/pinned-worktrees') {
        if (request.method() === 'PATCH') {
          const update = request.postDataJSON();
          state.pinnedWorktrees.paths = [...new Set([...state.pinnedWorktrees.paths, ...(update.add ?? [])])].filter(path => !update.remove?.includes(path));
          state.pinnedWorktrees.revision += 1;
        }
        data = state.pinnedWorktrees;
      }
      else if (url.pathname === '/api/aow/automations') data = [state.task];
      else if (url.pathname === detailPath) {
        data = { ...state.task };
        await state.detailGate;
      }
      else if (url.pathname === runsPath) data = [run];
      else if (url.pathname === '/api/aow/automations/status') data = { timezone: 'Asia/Shanghai', ready: true };
      else if (url.pathname === '/api/aow/agent-sessions') {
        state.sessionAgents.push(url.searchParams.get('agent'));
        data = (state.sessionFixtures ?? []).filter(session => session.agent === url.searchParams.get('agent'));
      }
      else if (url.pathname.endsWith('/notes/temporary')) {
        const owner = state.projects.find(project => url.pathname.includes(`/projects/${project.id}/`));
        const path = `${owner.notes_path}/.tmp-fixture.md`;
        state.textFiles[path] = { content: '', version: 'v1' };
        data = { path, name: '.tmp-fixture.md', kind: 'file' };
      }
      else if (url.pathname === '/api/fs/home') {
        data = { path: worktrees[0].path, entries: [{ name: 'edit.txt', path: `${worktrees[0].path}/edit.txt`, kind: 'file', readonly: false, size: 12, modified_ms: 1 }] };
      }
      else if (url.pathname === '/api/aow/pinned-directories') data = { paths: [], revision: 0 };
      else if (url.pathname.startsWith('/api/fs/tree')) {
        const path = decodeURIComponent(url.pathname.slice('/api/fs/tree'.length));
        data = { path, entries: Object.keys(state.textFiles).filter(file => file.slice(0, file.lastIndexOf('/')) === path).map(file => ({ name: file.split('/').at(-1), path: file, kind: 'file', size: 12, modified_ms: 1, readonly: false, hidden: false, is_symlink: false, link_target: null })) };
      } else if (url.pathname.startsWith('/api/fs/raw')) {
        await route.fulfill(state.failPreview ? { status: 404, body: 'Missing image' } : { contentType: 'image/svg+xml', body: `<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="${state.previewColor}"/></svg>` });
        return;
      } else if (url.pathname.startsWith('/api/fs/text')) {
        const path = decodeURIComponent(url.pathname.slice('/api/fs/text'.length));
        data = { path, ...state.textFiles[path], language: 'plaintext', mime: 'text/plain', size: 12 };
        await state.textGate;
        if (state.failText) { await route.fulfill({ status: 404, json: { message: 'File removed' } }); return; }
      } else if (url.pathname.startsWith('/api/fs/file') && request.method() === 'PUT') {
        const path = decodeURIComponent(url.pathname.slice('/api/fs/file'.length));
        state.writes.push({ path, content: request.postData(), version: request.headers()['if-match'] });
        if (request.headers()['if-match'] !== `"${state.textFiles[path].version}"`) {
          await route.fulfill({ status: 412, json: { message: 'Version conflict' } }); return;
        }
        state.textFiles[path] = { content: request.postData(), version: `${state.textFiles[path].version}-saved` };
        data = { path, size: request.postData().length, created: false, version: state.textFiles[path].version };
      }
      else if (url.pathname === '/api/terminals/agents') {
        state.terminalAgentScopes.push(url.searchParams.get('workspace_root'));
        if (state.agentFailure) { state.failedAgentRequests += 1; await route.fulfill({ status: 503, json: { message: 'Daemon unavailable' } }); return; }
        data = { agents: state.agents, titles: state.titles };
      }
      else if (url.pathname.startsWith('/api/terminals/') && request.method() === 'DELETE') {
        state.closedTerminalIds.push(url.pathname.split('/').at(-1));
        await route.fulfill({ status: 204 });
        return;
      }
      else if (url.pathname === '/api/terminals' && request.method() === 'POST') {
        const created = request.postDataJSON();
        state.createdTerminals.push(created);
        if (state.terminalCreateError) { await route.fulfill({ status: 500, json: { message: state.terminalCreateError } }); return; }
        const id = `created-${state.createdTerminals.length}`;
        const agent = state.registeredAgents.find(agent => agent.id === created.agent_id);
        data = { id, name: agent?.display_name ?? 'New shell', workspace_root: created.workspace_root, layout: { type: 'pane', pane_id: id + '-pane' }, panes: [{ id: id + '-pane', name: agent?.display_name ?? 'Shell', cwd: created.cwd, shell: agent?.command ?? '/bin/bash', kind: agent ? 'agent' : 'terminal', agent_id: agent ? agent.agent_type ?? agent.id : null, status: 'running', rows: 40, cols: 120 }] };
        state.createdTerminalTabs.push(data);
      }
      else if (url.pathname === '/api/terminals') {
        const scope = url.searchParams.get('workspace_root');
        state.terminalListScopes.push(scope);
        const roots = scope ? [scope] : [...new Set([...state.projects.flatMap(project => project.worktrees.map(worktree => worktree.path)), ...state.createdTerminalTabs.map(tab => tab.workspace_root)])];
        data = roots.flatMap(workspace => {
          const id = workspace.split('/').at(-1);
          let data = [{ id: `${id}-tab`, name: 'Shell', workspace_root: workspace, revision: 1, layout: { type: 'pane', pane_id: `${id}-pane` }, panes: [{ id: `${id}-pane`, name: 'Shell', cwd: workspace, shell: '/bin/bash', status: 'running', rows: 40, cols: 120 }] }];
          if (splitTerminal) {
            data[0].panes.push(...['review', 'logs'].map(suffix => ({ id: `${id}-${suffix}`, name: suffix, cwd: workspace, shell: '/bin/bash', status: 'running', rows: 40, cols: 120 })));
            data[0].layout = { type: 'split', axis: 'row', ratio: .5, first: { type: 'pane', pane_id: `${id}-pane` }, second: {
              type: 'split', axis: 'column', ratio: .5, first: { type: 'pane', pane_id: `${id}-review` }, second: { type: 'pane', pane_id: `${id}-logs` },
            } };
          }
          if (multipleTerminals) data.push({ id: `${id}-background`, name: 'Background', workspace_root: workspace, revision: 1, layout: { type: 'pane', pane_id: `${id}-background-pane` }, panes: [{ id: `${id}-background-pane`, name: 'Background', cwd: workspace, shell: '/bin/bash', status: 'running', rows: 40, cols: 120 }] });
          for (let index = 0; index < extraTerminals; index++) data.push({ id: `${id}-extra-${index}`, name: `Shell ${index + 2}`, workspace_root: workspace, revision: 1,
            layout: { type: 'pane', pane_id: `${id}-extra-${index}-pane` }, panes: [{ id: `${id}-extra-${index}-pane`, name: 'Shell', cwd: workspace, shell: '/bin/bash', status: 'running', rows: 40, cols: 120 }] });
          if (!state.defaultTerminalRoots.includes(workspace)) data = [];
          data.push(...state.createdTerminalTabs.filter(tab => tab.workspace_root === workspace));
          return data;
        });
        data = data.filter(tab => !state.closedTerminalIds.includes(tab.id));
        data = data.map(item => ({ ...item, name_is_custom: Boolean(state.names[item.id]), ...(state.names[item.id] ? { name: state.names[item.id] } : {}),
          panes: item.panes.map(pane => ({ ...pane, name_is_custom: Boolean(state.paneNames[pane.id]), ...(state.paneNames[pane.id] ? { name: state.paneNames[pane.id] } : {}) })),
        }));
        state.knownTerminalTabs = { ...state.knownTerminalTabs, ...Object.fromEntries(data.map(item => [item.id, item])) };
        if (!scope) await state.allTerminalsGate;
      }
      else if (request.method() === 'GET' && url.pathname.startsWith('/api/terminals/')) {
        data = state.knownTerminalTabs?.[url.pathname.split('/').at(-1)];
      }
      else if (request.method() === 'PATCH' && url.pathname.startsWith('/api/terminals/')) {
        const names = url.pathname.includes('/panes/') ? state.paneNames : state.names;
        names[url.pathname.split('/').at(-1)] = request.postDataJSON().name;
        data = null; // Exercise the client's successful-rename fallback too.
      }
      else if (url.pathname === '/api/git/ignored') data = { repository: '/workspace', ignored: [] };
      else if (url.pathname === '/api/git/repositories') data = [];
      else if (url.pathname === '/api/git/status') data = { repository: url.searchParams.get('repository'), branch: 'main', files: [0, 1, 2].map(i => ({ path: `file-${i}.txt`, index_status: state.staged && i === 0 ? 'M' : ' ', worktree_status: 'M', original_path: null })) };
      else if (url.pathname === '/api/git/diff' || url.pathname === '/api/git/commit/diff') {
        state.diffRequests.push(Object.fromEntries(url.searchParams));
        const file = state.diffFiles[Number(url.searchParams.get('path').match(/file-(\d+)/)[1])];
        data = { repository: url.searchParams.get('repository'), path: url.searchParams.get('path'), ...(!fallback && { original: file.original, modified: file.modified }), patch: file.patch, truncated: false };
        await state.diffGate;
      }
      else if (url.pathname === '/api/git/commit/files') data = { repository: '/workspace/wt-0', commit: 'commit-1', files: [{ path: 'file-0.txt', status: 'R', original_path: 'old.txt' }] };
      else if (url.pathname === '/api/git/log') data = { commits: state.commits, repository: '/workspace', upstream: null, upstream_commit: null };
      await route.fulfill(data === undefined ? { status: 404, json: { message: `Fixture absent: ${url.pathname}` } } : { json: data });
    });
    await context.routeWebSocket('**/api/terminals/**', socket => {
      state.terminalSockets.push(socket);
      socket.onClose(() => state.closedTerminalConnections.push(socket.url()));
      socket.onMessage(async message => {
        state.terminalMessages.push({ url: socket.url(), message });
        if (typeof message !== 'string' || JSON.parse(message).type !== 'claim') return;
        if (state.terminalClaimGate) await state.terminalClaimGate;
        const control = JSON.parse(message);
        const tab = state.createdTerminalTabs.find(tab => socket.url().includes(`/terminals/${tab.id}/`));
        const cli = tab?.panes[0].agent_terminal;
        socket.send(JSON.stringify({ type: 'control', state: cli && (!control.force || cli.phase !== 'ready') ? 'observing' : 'claimed' }));
        socket.send(JSON.stringify({ type: 'stream', epoch: 'fixture', offset: 0, reset: true, replay_bytes: 0, restore_cols: cli ? 160 : 120, restore_rows: cli ? 48 : 40, restore: '\x1b[2J\x1b[HFixture ready\r\n$ ' }));
      });
    });
    const page = await context.newPage();
    page.setDefaultTimeout(15000);
    page.on('pageerror', error => state.errors.push(error.stack ?? error.message));
    if (clock) await page.clock.install({ time: '2026-09-15T00:00:00Z' });
    await beforeOpen?.({ context, state });
    await page.goto(`${baseURL}/?ui=desktop`);
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').first().waitFor();
    return { page, state };
  }

  const groupButton = (root, name) => root.getByRole('button', { name: new RegExp(`^(展开|收起) ${name} 分组`) });

  await test('project avatars load independently, directly reference the image and recover from broken or unsupported providers', async t => {
    let releaseAvatar;
    const gate = new Promise(resolve => { releaseAvatar = resolve; });
    t.after(() => releaseAvatar());
    const imageRequests = [];
    const { page, state } = await fixture(t, { beforeOpen: async ({ context, state }) => {
      state.avatarGate = gate;
      state.avatarUrl = 'https://avatars.example.com/owner.svg';
      await context.route('https://avatars.example.com/**', route => {
        imageRequests.push(route.request());
        return route.request().url().includes('broken') ? route.abort() : route.fulfill({ contentType: 'image/svg+xml', body: '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect width="64" height="64" rx="12" fill="#3267cf"/><circle cx="32" cy="24" r="10" fill="white"/><path d="M12 58a20 20 0 0 1 40 0" fill="white"/></svg>' });
      });
    } });
    const header = page.locator('.project-aow-project-row').first();
    assert.equal(await header.locator('svg.project-icon').isVisible(), true, 'project and terminals render while avatar metadata is pending');
    releaseAvatar();
    const avatar = header.locator('img.project-icon');
    await avatar.waitFor();
    await eventually(() => avatar.evaluate(image => image.complete && image.naturalWidth > 0));
    assert.equal(await avatar.getAttribute('src'), state.avatarUrl);
    assert.equal(await avatar.getAttribute('referrerpolicy'), 'no-referrer');
    assert.equal(imageRequests[0].resourceType(), 'image');
    assert.equal(imageRequests[0].headers().referer, undefined);
    assert.deepEqual(await avatar.evaluate(image => [image.clientWidth, image.clientHeight]), [14, 14]);
    if (process.env.AOW_PROJECT_ICON_SCREENSHOT) await page.screenshot({ path: process.env.AOW_PROJECT_ICON_SCREENSHOT });

    state.projects = state.projects.map(project => ({ ...project, avatar_url: state.avatarUrl }));
    state.avatarFailure = true;
    await page.reload();
    await avatar.waitFor();
    assert.equal(await avatar.getAttribute('src'), state.avatarUrl, 'saved URL remains visible when metadata is unavailable');
    state.avatarFailure = false;
    state.avatarUrl = 'https://avatars.example.com/broken.svg';
    await page.evaluate(() => window.dispatchEvent(new Event('aow-review-providers-changed')));
    await eventually(() => Promise.resolve(imageRequests.some(request => request.url().includes('broken'))));
    await header.locator('svg.project-icon').waitFor();
    assert.equal(await avatar.count(), 0, 'a broken image returns to the folder icon');

    state.avatarUrl = null;
    const refreshed = page.waitForResponse(response => response.url().endsWith('/projects/project/avatar'));
    await page.evaluate(() => window.dispatchEvent(new Event('aow-review-providers-changed')));
    await refreshed;
    await header.locator('svg.project-icon').waitFor();
    assert.deepEqual(state.errors, []);
  });

  async function registrationFixture(t) {
    const result = await fixture(t, { beforeOpen: async ({ context, state }) => {
      state.registrations = [];
      state.directoryReads = [];
      state.deniedDirectories = new Set();
      const directory = path => ({ name: path.split('/').at(-1), path, kind: 'directory' });
      const directories = new Map([
        ['/', [directory('/workspace')]],
        ['/workspace', [...worktrees.map(worktree => directory(worktree.path)), { name: 'file.txt', path: '/workspace/file.txt', kind: 'file' }]],
        ...worktrees.map(worktree => [worktree.path, []]),
      ]);
      await context.route(/\/api\/fs\/(home|tree.*)$/, async route => {
        const pathname = new URL(route.request().url()).pathname;
        const path = pathname === '/api/fs/home' ? '/workspace' : decodeURIComponent(pathname.slice('/api/fs/tree'.length)).replace(/\/+$/, '') || '/';
        state.directoryReads.push(path);
        await state.directoryGate;
        if (!directories.has(path) || state.deniedDirectories.has(path)) {
          await route.fulfill({ status: 403, json: { message: '无法读取目录' } });
          return;
        }
        await route.fulfill({ json: { path, entries: directories.get(path) } });
      });
      await context.route('**/api/aow/projects', async route => {
        if (route.request().method() !== 'POST') { await route.fallback(); return; }
        const payload = route.request().postDataJSON();
        state.registrations.push(payload);
        await state.registrationGate;
        if (state.failRegistration) {
          await route.fulfill({ status: 400, json: { message: '所选目录不是 Git 仓库' } });
          return;
        }
        const registered = { ...project, registered_path: payload.path, name: payload.name || payload.path.split('/').at(-1), notes_path: payload.notes_path || project.notes_path };
        state.projects = [registered];
        await route.fulfill({ json: registered });
      });
    } });
    await result.page.getByRole('button', { name: '注册项目', exact: true }).click();
    const dialog = result.page.getByRole('dialog', { name: '注册项目', exact: true });
    await dialog.getByRole('button', { name: 'wt-0', exact: true }).waitFor();
    return { ...result, dialog };
  }

  await test('project registration selects a directory before entering details and preserves edits when going back', async t => {
    const { page, state, dialog } = await registrationFixture(t);
    assert.equal(await dialog.getByRole('region', { name: '选择仓库目录' }).isVisible(), true);
    assert.equal(await dialog.getByRole('textbox').count(), 1);
    assert.equal(await dialog.getByRole('button', { name: /^(浏览|选择当前目录|注册项目|file.txt)$/ }).count(), 0);
    assert.match(await dialog.locator('[aria-current="step"]').textContent(), /选择仓库目录/);
    await dialog.getByRole('button', { name: 'wt-0', exact: true }).click();
    await dialog.getByRole('button', { name: '下一步', exact: true }).click();
    await dialog.getByLabel('显示名称').waitFor();
    assert.equal(await dialog.locator('.project-aow-register-preview code').textContent(), worktrees[0].path);
    assert.equal(await dialog.getByRole('textbox', { name: '目录路径', exact: true }).count(), 0);
    assert.deepEqual(state.registrations, [], 'selecting a directory must not register it');
    assert.equal(await dialog.getByLabel('显示名称').evaluate(node => node === document.activeElement), true);
    await dialog.getByLabel('显示名称').fill('项目名称');
    await dialog.getByLabel('Notes 路径').fill('/notes/custom');
    await dialog.getByRole('button', { name: '上一步', exact: true }).click();
    assert.equal(await dialog.getByRole('textbox', { name: '目录路径', exact: true }).inputValue(), worktrees[0].path);
    await dialog.getByRole('button', { name: '上级目录', exact: true }).click();
    await dialog.getByRole('button', { name: 'wt-1', exact: true }).click();
    await dialog.getByRole('button', { name: '下一步', exact: true }).click();
    await dialog.getByLabel('显示名称').waitFor();
    assert.equal(await dialog.getByLabel('显示名称').inputValue(), '项目名称');
    assert.equal(await dialog.getByLabel('Notes 路径').inputValue(), '/notes/custom');
    assert.equal(await dialog.locator('.project-aow-register-preview code').textContent(), worktrees[1].path);

    state.failRegistration = true;
    await dialog.getByRole('button', { name: '注册项目', exact: true }).click();
    await dialog.getByRole('alert').waitFor();
    assert.match(await dialog.getByRole('alert').textContent(), /不是 Git 仓库/);
    assert.equal(await dialog.getByLabel('显示名称').inputValue(), '项目名称');
    assert.deepEqual(state.registrations, [{ path: worktrees[1].path, name: '项目名称', notes_path: '/notes/custom' }]);
    state.failRegistration = false;
    let finishRegistration;
    state.registrationGate = new Promise(resolve => { finishRegistration = resolve; });
    t.after(() => finishRegistration());
    await dialog.getByRole('button', { name: '注册项目', exact: true }).click();
    await dialog.getByRole('button', { name: '正在注册…', exact: true }).waitFor();
    assert.equal(await dialog.getByRole('button', { name: '上一步', exact: true }).isDisabled(), true);
    assert.equal(await dialog.getByRole('button', { name: '取消', exact: true }).isDisabled(), true);
    await page.keyboard.press('Escape');
    assert.equal(await dialog.isVisible(), true);
    finishRegistration();
    await dialog.waitFor({ state: 'hidden' });
    assert.equal(state.registrations.length, 2);
  });

  await test('project registration validates pasted paths before advancing and submits defaults only in the details step', async t => {
    const { page, state, dialog } = await registrationFixture(t);
    const address = dialog.getByRole('textbox', { name: '目录路径', exact: true });
    const next = dialog.getByRole('button', { name: '下一步', exact: true });
    await address.fill('relative/path');
    await next.click();
    await dialog.getByRole('alert').waitFor();
    assert.match(await dialog.getByRole('alert').textContent(), /绝对目录路径/);
    assert.equal(await next.isDisabled(), true);
    await address.fill('/missing');
    await next.click();
    await dialog.getByRole('alert').waitFor();
    assert.equal(await dialog.getByRole('alert').textContent(), '无法读取目录');
    assert.equal(await next.isDisabled(), true);
    assert.equal(await dialog.getByLabel('显示名称').count(), 0);
    assert.deepEqual(state.registrations, []);

    let finishDirectory;
    state.directoryGate = new Promise(resolve => { finishDirectory = resolve; });
    t.after(() => finishDirectory());
    await address.fill(`  ${worktrees[1].path}/  `);
    await next.click();
    await dialog.locator('[aria-busy="true"]').waitFor();
    assert.equal(await next.isDisabled(), true);
    assert.equal(await dialog.getByLabel('显示名称').count(), 0);
    finishDirectory();
    await dialog.getByLabel('显示名称').waitFor();
    assert.equal(await dialog.locator('.project-aow-register-preview code').textContent(), worktrees[1].path);
    assert.equal(await dialog.locator('.project-aow-register-preview strong').textContent(), 'wt-1');
    assert.equal(await dialog.getByLabel('Notes 路径').inputValue(), '');
    assert.deepEqual(state.registrations, []);
    assert.ok(state.directoryReads.includes(worktrees[1].path));
    await dialog.getByRole('button', { name: '注册项目', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.deepEqual(state.registrations, [{ path: worktrees[1].path }]);
    await page.getByRole('button', { name: '注册项目', exact: true }).click();
    await dialog.getByRole('button', { name: 'wt-0', exact: true }).waitFor();
    assert.equal(await address.inputValue(), '/workspace', 'reopening starts a fresh directory selection');
    await page.keyboard.press('Escape');
    await dialog.waitFor({ state: 'hidden' });
  });

  await test('project registration keeps address navigation in the first step and recovers from directory errors', async t => {
    const { page, state, dialog } = await registrationFixture(t);
    await page.setViewportSize({ width: 640, height: 600 });
    const address = dialog.getByRole('textbox', { name: '目录路径', exact: true });
    await address.fill(worktrees[0].path);
    await address.press('Enter');
    await dialog.locator('.project-aow-directory-list[aria-busy="false"]').waitFor();
    assert.equal(await dialog.getByLabel('显示名称').count(), 0);
    assert.equal(await address.inputValue(), worktrees[0].path);
    state.deniedDirectories.add(worktrees[0].path);
    await dialog.getByRole('button', { name: '刷新当前目录', exact: true }).click();
    await dialog.getByRole('alert').waitFor();
    assert.equal(await dialog.getByRole('button', { name: '下一步', exact: true }).isDisabled(), true);
    await dialog.getByRole('button', { name: '服务端根目录', exact: true }).click();
    await dialog.getByRole('button', { name: 'workspace', exact: true }).waitFor();
    assert.equal(await address.inputValue(), '/');
    assert.equal(await dialog.getByRole('button', { name: '下一步', exact: true }).isEnabled(), true);
    await address.fill(worktrees[1].path);
    await dialog.getByRole('button', { name: '转到', exact: true }).click();
    await dialog.locator('.project-aow-directory-list[aria-busy="false"]').waitFor();
    assert.equal(await dialog.getByLabel('显示名称').count(), 0);
    assert.equal(await dialog.evaluate(node => node.scrollWidth <= node.clientWidth && node.getBoundingClientRect().bottom <= window.innerHeight), true);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.deepEqual(state.registrations, []);
  });

  await test('project panels share aligned actions and preserve manual collapse across refreshes', async t => {
    const { page, state } = await fixture(t, { beforeOpen: ({ state }) => {
      state.projects = [project, { ...project, id: 'empty-project', name: 'Empty Project', worktrees: [] }];
    } });
    const sidebar = page.getByRole('complementary', { name: '项目侧边栏', exact: true });
    const projectPanel = sidebar.getByRole('region', { name: project.name, exact: true });
    const toggle = projectPanel.getByRole('button', { name: /^(展开|收起) Resource Fixture$/ });
    const emptyToggle = sidebar.getByRole('button', { name: /^(展开|收起) Empty Project$/ });
    assert.equal(await emptyToggle.getAttribute('aria-expanded'), 'false');
    const actions = await sidebar.locator('.project-aow-project-list-title .aow-icon-button, .project-aow-project-row .aow-icon-button').evaluateAll(buttons => buttons
      .filter(button => !button.classList.contains('aow-panel-toggle'))
      .map(button => { const rect = button.getBoundingClientRect(); return { x: rect.x, width: rect.width, height: rect.height }; }));
    assert.equal(actions.length, 6);
    assert.deepEqual(actions.slice(0, 2), actions.slice(2, 4));
    assert.deepEqual(actions.slice(0, 2), actions.slice(4, 6));
    assert.ok(actions.every(button => button.width === 22 && button.height === 22));
    await toggle.click();
    state.projects[1] = { ...state.projects[1], worktrees: [{ ...worktrees[0], project_id: 'empty-project', id: 'empty-main', path: '/workspace/empty' }] };
    await sidebar.getByRole('button', { name: '刷新全部', exact: true }).click();
    await eventually(async () => await emptyToggle.getAttribute('aria-expanded') === 'true');
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    assert.equal(await projectPanel.locator('.project-aow-worktrees').isVisible(), false);
    await sidebar.getByRole('button', { name: '收起 Projects', exact: true }).click();
    assert.equal(await projectPanel.isVisible(), false);
    await sidebar.getByRole('button', { name: '定位 Projects', exact: true }).click();
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    await sidebar.getByRole('button', { name: `定位 ${project.name}`, exact: true }).click();
    assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
    await projectPanel.locator('.project-aow-worktrees').waitFor();
  });

  await test('worktree cleanup filters only in the dialog, confirms dirty items and tracks mixed background results after closing', async t => {
    const { page, state } = await fixture(t, { beforeOpen: ({ state }) => {
      state.projects = [{ ...project, worktrees: [...worktrees, { ...worktrees[1], id: 'wt-2', path: '/workspace/wt-2', branch: 'branch-2' }] }];
      state.dirtyWorktrees = [worktrees[1].path];
      state.defaultTerminalRoots = [worktrees[0].path, '/workspace/wt-2'];
    } });
    await eventually(async () => await page.locator('.project-aow-worktrees button.compact').count() === 1);
    assert.equal(await page.getByLabel('仅显示未分配资源', { exact: true }).count(), 0);
    await page.getByRole('button', { name: /Project 操作$/ }).click();
    await page.getByRole('menuitem', { name: '批量清理 Worktree', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '批量清理 Worktree', exact: true });
    await dialog.getByLabel('仅显示未分配资源', { exact: true }).check();
    assert.equal(await dialog.locator('.worktree-cleanup-row').count(), 1);
    await dialog.getByLabel('选择 branch-1', { exact: true }).check();
    await dialog.getByLabel('仅显示未分配资源', { exact: true }).uncheck();
    assert.equal(await dialog.getByLabel('选择 branch-1', { exact: true }).isChecked(), false, 'changing filters clears hidden selections');
    assert.equal(await dialog.getByLabel('选择 branch-0', { exact: true }).isDisabled(), true);
    await dialog.getByLabel('全选当前结果', { exact: true }).check();
    await dialog.getByRole('button', { name: '检查并确认清理', exact: true }).click();
    const confirm = dialog.getByRole('button', { name: '确认清理 2 个 Worktree', exact: true });
    await confirm.waitFor();
    assert.equal(await confirm.isDisabled(), true);
    await dialog.getByText('查看将删除的未提交内容').click();
    await dialog.getByText('?? uncommitted.txt', { exact: true }).waitFor();
    await dialog.getByRole('checkbox', { name: /我确认强制删除/ }).check();
    await confirm.click();
    await eventually(async () => state.removalSubmissions.length === 1);
    assert.deepEqual(state.removalSubmissions[0], [{ path: '/workspace/wt-1', force: true }, { path: '/workspace/wt-2', force: false }]);
    await dialog.getByRole('button', { name: '关闭', exact: true }).first().click();
    assert.equal(await page.locator('.project-aow-worktrees button[title="/workspace/wt-1"]').isDisabled(), true);
    await page.getByRole('button', { name: '删除 Worktree · 0/2', exact: true }).click();
    await dialog.waitFor();
    state.removalJobs[0].items[0].status = 'succeeded';
    state.removalJobs[0].items[1].status = 'failed';
    state.removalJobs[0].items[1].error = 'Permission denied';
    state.projects = state.projects.map(project => ({ ...project, worktrees: project.worktrees.filter(worktree => worktree.path !== '/workspace/wt-1') }));
    await eventually(async () => await page.locator('.project-aow-worktrees button[title="/workspace/wt-1"]').count() === 0);
    assert.equal(await page.getByRole('button', { name: '查看清理结果', exact: true }).count(), 0);
    await dialog.getByText('已删除', { exact: true }).waitFor();
    await dialog.getByText('Permission denied', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: '重新检查并重试', exact: true }).click();
    await dialog.getByRole('button', { name: '确认清理 1 个 Worktree', exact: true }).click();
    await eventually(async () => state.removalSubmissions.length === 2);
    assert.deepEqual(state.removalSubmissions[1], [{ path: '/workspace/wt-2', force: false }]);
    await page.screenshot({ path: '/tmp/worktree-cleanup-progress.png' });
  });

  await test('single worktree deletion submits a background task and restores status after page reload', async t => {
    const { page, state } = await fixture(t);
    await page.locator('.project-aow-worktrees button[title="/workspace/wt-1"]').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '删除 Worktree', exact: true }).click();
    const single = page.getByRole('dialog', { name: '删除 Worktree', exact: true });
    await single.getByRole('button', { name: '删除 Worktree', exact: true }).click();
    await eventually(async () => state.removalSubmissions.length === 1);
    await page.getByRole('dialog', { name: '批量清理 Worktree', exact: true }).getByText('排队中', { exact: true }).first().waitFor();
    await page.reload();
    await page.getByRole('button', { name: '删除 Worktree · 0/1', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: '批量清理 Worktree', exact: true });
    await dialog.getByText('排队中', { exact: true }).first().waitFor();
    assert.equal(await dialog.getByLabel('选择 branch-1', { exact: true }).isDisabled(), true);
    assert.equal(state.removalSubmissions.length, 1, 'reload never replays deletion');
  });

  await test('shared operation bar discovers CLI work and paginates empty filtered log pages', async t => {
    const event = (message, id = 'cli-op') => ({ timestamp: '2026-09-23T12:00:00Z', operation_id: id, boot_id: 'fixture',
      kind: 'agent.create', source: 'cli', title: '创建 Agent · codex', event: 'progress', level: 'info', message });
    const { page, state } = await fixture(t, { beforeOpen: ({ state }) => {
      state.otherOperations = [{ id: 'cli-op', kind: 'agent.create', source: 'cli', title: '创建 Agent · codex', project_id: 'project',
        resource: '/aow/tabs/terminal/cli-tab', created_at: '2026-09-23T12:00:00Z', message: '等待 Agent 就绪', completed: 0, total: null, outcome: null }];
      state.readLogs = query => {
        const offset = query.cursor ? JSON.parse(query.cursor).offset : undefined;
        return query.query === 'older' && offset === undefined
          ? { items: [], next_cursor: { file: 'operations.2026-09-23-12.log', offset: 100 }, budget_exhausted: true, scanned_bytes: 4194304 }
          : { items: [event(offset === 100 ? 'older matching result' : '最新启动记录')], next_cursor: null, budget_exhausted: false, scanned_bytes: 500 };
      };
    } });
    await page.getByRole('button', { name: '创建 Agent · codex', exact: true }).click();
    const panel = page.getByRole('region', { name: '操作日志', exact: true });
    await panel.getByText('最新启动记录', { exact: true }).waitFor();
    assert.equal(await panel.getByRole('link', { name: '查看 Terminal' }).getAttribute('href'), '/aow/tabs/terminal/cli-tab');
    await panel.getByRole('button', { name: '关闭操作日志' }).click();
    assert.equal(await page.getByRole('button', { name: '创建 Agent · codex', exact: true }).count(), 1);
    await page.getByRole('button', { name: '操作日志', exact: true }).click();
    await panel.getByLabel('搜索操作日志').fill('older');
    await panel.getByRole('button', { name: '搜索', exact: true }).click();
    await panel.getByText('这部分日志没有匹配记录，可以继续查找。').waitFor();
    await panel.getByRole('button', { name: '继续查找', exact: true }).click();
    await panel.getByText('older matching result', { exact: true }).waitFor();
    assert.equal(JSON.parse(state.logQueries.at(-1).cursor).offset, 100);
    state.otherOperations[0].outcome = 'succeeded';
    state.otherOperations[0].message = 'Agent 已就绪';
    await eventually(async () => await page.locator('.operation-status-task').count() === 0);
    assert.equal(await panel.getByText('older matching result', { exact: true }).count(), 1, 'live updates preserve the historical page');
    await panel.getByLabel('操作来源').selectOption('cli');
    await eventually(async () => state.logQueries.at(-1).source === 'cli' && !state.logQueries.at(-1).cursor);
    await panel.getByRole('button', { name: '关闭操作日志' }).click();
    await page.reload();
    assert.equal(await page.locator('.operation-status-task').count(), 0);
    await page.getByRole('button', { name: '操作日志', exact: true }).click();
    await page.getByRole('region', { name: '操作日志', exact: true }).getByText('最新启动记录', { exact: true }).waitFor();
    assert.equal(await page.getByRole('button', { name: '查看清理结果', exact: true }).count(), 0);
    await page.screenshot({ path: '/tmp/aow-operation-logs.png' });
  });

  await test('empty worktrees use compact rows while main, pinned and opened resources retain full rows', async t => {
    const paths = Array.from({ length: 5 }, (_, i) => `/workspace/wt-${i}`);
    const { page, state } = await fixture(t, { floating: true, beforeOpen: ({ state }) => {
      state.projects[1] = { ...project, worktrees: paths.map((path, i) => ({ ...worktrees[1], id: `wt-${i}`, path, branch: `branch-${i}`, is_main: i === 0, icon: 'cat', color: 'purple' })) };
      state.defaultTerminalRoots = paths;
      state.closedTerminalIds.push('wt-1-tab', 'wt-3-tab', 'wt-4-tab');
      state.pinnedWorktrees.paths = [paths[4]];
    } });
    const list = page.locator('.project-aow-worktrees');
    const row = i => list.locator(`button[title="${paths[i]}"]`);
    const isCompact = i => row(i).evaluate(element => element.classList.contains('compact'));
    const order = () => list.locator(':scope > button').evaluateAll(rows => rows.map(row => row.title));
    await eventually(async () => await isCompact(1) && await isCompact(3));
    assert.deepEqual(await order(), [paths[0], paths[2], paths[4], paths[1], paths[3]]);
    assert.equal(await row(1).textContent(), 'branch-1');
    assert.equal(await row(1).locator('svg, strong, small').count(), 0);
    assert.equal(await row(1).evaluate(element => element.getBoundingClientRect().height), 22);
    assert.equal(await row(1).locator('.project-aow-worktree-branch').evaluate(element => getComputedStyle(element).fontWeight), '400');
    assert.equal(await row(1).locator('.project-aow-worktree-dot').evaluate(element => getComputedStyle(element, '::before').backgroundColor), 'rgb(115, 123, 135)');
    assert.equal(await page.locator('.project-aow-surface').count(), 1, 'inventory must not mount unopened workspaces');
    assert.ok(state.terminalSockets.every(socket => socket.url().includes('/terminals/wt-0-tab/')), 'inventory must not attach to unopened terminals');

    await surface(page).getByRole('button', { name: '关闭 Shell', exact: true }).click();
    await surface(page).locator('.project-aow-welcome').waitFor();
    state.defaultTerminalRoots = paths.slice(1);
    await surface(page).getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await eventually(async () => await surface(page).locator('.terminal-panel-open').count() === 0);
    assert.equal(await isCompact(0), false, 'the empty main worktree keeps its full row');
    await row(4).locator('svg.lucide-cat').waitFor();
    await page.locator('.project-aow-pinned').locator('svg.lucide-cat').waitFor();
    await row(1).click();
    await surface(page).locator('.project-aow-welcome').waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), paths[1]);
    assert.equal(await isCompact(1), true, 'selection alone does not allocate a resource');

    const menu = page.getByRole('menu', { name: `${paths[1]} 操作`, exact: true });
    await row(1).click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Pin to top', exact: true }).click();
    await row(1).locator('svg.lucide-cat').waitFor();
    await eventually(() => state.pinnedWorktrees.paths.includes(paths[1]));
    await row(1).click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Unpin', exact: true }).click();
    await eventually(() => isCompact(1));
    await row(0).click();
    await row(1).focus();
    await page.keyboard.press('Enter');
    await eventually(async () => await page.evaluate(() => localStorage.getItem('aow-active')) === paths[1]);

    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    const file = surface(page).locator(`.tree-row[data-tree-path="${paths[1]}/edit.txt"]`);
    await file.click();
    await tab(page, 'edit.txt').waitFor();
    await row(1).locator('svg.lucide-cat').waitFor();
    assert.deepEqual(await order(), [paths[0], paths[1], paths[2], paths[4], paths[3]]);
    assert.equal(await row(1).locator('svg').evaluate(element => getComputedStyle(element).color), 'rgb(168, 85, 247)');
    await surface(page).getByRole('button', { name: '关闭 edit.txt', exact: true }).click();
    await eventually(() => isCompact(1));
    assert.deepEqual(await order(), [paths[0], paths[2], paths[4], paths[1], paths[3]]);

    await file.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    await row(1).locator('svg.lucide-cat').waitFor();
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await row(0).click();
    assert.equal(await isCompact(1), false, 'hidden and floating resources still belong to the worktree');
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    await panel.getByRole('button', { name: '关闭 edit.txt', exact: true }).click();
    await eventually(() => isCompact(1));
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    if (process.env.COMPACT_WORKTREE_SCREENSHOT) await page.screenshot({ path: process.env.COMPACT_WORKTREE_SCREENSHOT });
    assert.deepEqual(state.closedTerminalIds, ['wt-1-tab', 'wt-3-tab', 'wt-4-tab'], 'closing views must not destroy terminal instances');
  });

  await test('unvisited worktree rows count frontend tabs and all backend terminal instances after reload', async t => {
    const paths = Array.from({ length: 9 }, (_, i) => `/workspace/wt-${i}`);
    const { page, state } = await fixture(t, { beforeOpen: async ({ context, state }) => {
      state.projects = [{ ...project, worktrees: paths.map((path, i) => ({ ...worktrees[1], id: `wt-${i}`, path, branch: `branch-${i}`, is_main: i === 0 })) }];
      state.defaultTerminalRoots = paths;
      state.closedTerminalIds.push(...[1, 3, 4, 5, 6, 7, 8].map(i => `wt-${i}-tab`));
      state.createdTerminalTabs.push(...[1, 7].map(i => ({ id: `cli-${i}`, name: 'CLI', workspace_root: paths[i], revision: 1,
        layout: { type: 'pane', pane_id: `cli-${i}-pane` }, panes: [{ id: `cli-${i}-pane`, name: 'CLI', cwd: paths[i], shell: '/bin/codex', status: 'running', rows: 40, cols: 120,
          agent_terminal: { phase: 'ready', error: null, task_submitted: false } }] })));
      await context.addInitScript(({ paths, task }) => {
        const saved = {
          1: { terminalVisibility: { 'cli-1': false } },
          2: { terminalVisibility: { 'wt-2-tab': false } },
          3: { documents: [{ id: `${paths[3]}/edit.txt`, path: `${paths[3]}/edit.txt` }] },
          4: { sessions: [{ session: { id: 'session' }, workspacePath: paths[4] }] },
          5: { pullRequests: [{ number: 42 }] },
          6: { tasks: [task] },
          7: { terminalVisibility: { 'cli-7': true } },
        };
        for (const [index, value] of Object.entries(saved)) {
          const key = `aow-workspace-tabs:${paths[index]}`;
          if (!localStorage.getItem(key)) localStorage.setItem(key, JSON.stringify(value));
        }
      }, { paths, task });
    } });
    const list = page.locator('.project-aow-worktrees');
    const compactPaths = () => list.locator('button.compact').evaluateAll(rows => rows.map(row => row.title));
    await eventually(async () => (await compactPaths()).length === 1);
    assert.deepEqual(await compactPaths(), [paths[8]]);
    assert.equal(await page.locator('.project-aow-surface').count(), 1);
    assert.ok(state.terminalSockets.every(socket => socket.url().includes('/terminals/wt-0-tab/')));
    await page.reload();
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    await eventually(async () => (await compactPaths()).length === 1);
    assert.deepEqual(await compactPaths(), [paths[8]]);
    assert.equal(state.requests.some(path => path.startsWith(`/api/fs/text${paths[3]}`)), false, 'reading saved resource presence must not restore unopened editors');
    await list.locator(`button[title="${paths[2]}"]`).click();
    await surface(page).locator('.project-aow-welcome').waitFor();
    assert.equal(await surface(page).getByRole('tab').count(), 0, 'a hidden terminal remains closed when switching worktrees');
    await surface(page).locator('.terminal-panel-open').filter({ hasText: 'Shell' }).click();
    await tab(page, 'Shell').waitFor();
    await eventually(async () => !(await compactPaths()).includes(paths[2]));
    await surface(page).getByRole('button', { name: '关闭 Shell', exact: true }).click();
    await surface(page).locator('.project-aow-welcome').waitFor();
    assert.equal((await compactPaths()).includes(paths[2]), false, 'a hidden user terminal is still a backend resource');
    state.closedTerminalIds.push('wt-2-tab');
    await surface(page).getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await eventually(async () => (await compactPaths()).includes(paths[2]));
  });

  for (const origin of ['user', 'CLI']) {
    await test(`${origin} terminal instances keep worktrees expanded while hidden or exited and update after removal`, async t => {
      const instance = { id: 'external-terminal', name: `External ${origin}`, workspace_root: worktrees[1].path, revision: 1,
        layout: { type: 'pane', pane_id: 'external-terminal-pane' }, panes: [{ id: 'external-terminal-pane', name: 'Terminal', cwd: worktrees[1].path,
          shell: '/bin/bash', status: 'running', rows: 40, cols: 120,
          ...(origin === 'CLI' ? { agent_terminal: { phase: 'ready', error: null, task_submitted: false } } : {}) }] };
      const { page, state } = await fixture(t, { clock: true, beforeOpen: async ({ context, state }) => {
        state.closedTerminalIds.push('wt-1-tab');
        await context.addInitScript(({ path, id }) => {
          const key = `aow-workspace-tabs:${path}`;
          if (!localStorage.getItem(key)) localStorage.setItem(key, JSON.stringify({ terminalVisibility: { [id]: false } }));
        }, { path: worktrees[1].path, id: instance.id });
      } });
      const row = page.locator(`.project-aow-worktrees button[title="${worktrees[1].path}"]`);
      const isCompact = () => row.evaluate(element => element.classList.contains('compact'));
      const refresh = () => page.clock.runFor(3100);
      await eventually(isCompact);
      state.createdTerminalTabs.push(instance);
      await refresh();
      await eventually(async () => !await isCompact());
      assert.equal(await page.locator('.project-aow-surface').count(), 1, 'discovering resources does not mount their workspace');
      assert.equal(state.terminalSockets.some(socket => socket.url().includes(`/terminals/${instance.id}/`)), false, 'inventory must not attach to terminals');
      state.createdTerminalTabs = [];
      await refresh();
      await eventually(isCompact);
      state.createdTerminalTabs.push(instance);
      await refresh();
      await eventually(async () => !await isCompact());

      await row.click();
      await surface(page).locator('.terminal-panel-open').filter({ hasText: instance.name }).waitFor();
      await surface(page).locator('.project-aow-welcome').waitFor();
      assert.equal(await surface(page).getByRole('tab').count(), 0, 'counting a backend resource does not open its tab');
      assert.equal(await isCompact(), false, 'the live workspace also counts hidden terminal instances');
      await surface(page).locator('.terminal-panel-open').filter({ hasText: instance.name }).click();
      await surface(page).locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
      await surface(page).getByRole('button', { name: `关闭 ${instance.name}`, exact: true }).click();
      await eventually(async () => await page.evaluate(({ path, id }) => JSON.parse(localStorage.getItem(`aow-workspace-tabs:${path}`)).terminalVisibility[id] === false,
        { path: worktrees[1].path, id: instance.id }));
      assert.equal(await isCompact(), false, 'hiding a tab does not release its terminal instance');
      await page.reload();
      await surface(page).locator('.terminal-panel-open').filter({ hasText: instance.name }).waitFor();
      await surface(page).locator('.project-aow-welcome').waitFor();
      assert.equal(await isCompact(), false, 'a hidden terminal instance still counts after reload');
      assert.equal(await surface(page).getByRole('tab').count(), 0);
      instance.panes[0].status = 'exited';
      await refresh();
      await surface(page).locator('.terminal-panel-row').filter({ hasText: instance.name }).getByText('已退出', { exact: true }).waitFor();
      assert.equal(await isCompact(), false, 'process exit does not delete the backend terminal instance');
      await switchWorktree(page, 0);
      await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
      state.createdTerminalTabs = [];
      await refresh();
      await eventually(isCompact);
      const globalRequests = state.terminalListScopes.filter(scope => scope === null).length;
      await refresh();
      assert.equal(state.terminalListScopes.filter(scope => scope === null).length, globalRequests, 'mounted workspaces reuse their existing terminal polling');
    });
  }

  await test('worktrees stay expanded while the backend inventory is loading or unavailable', async t => {
    let release;
    let failInventory = false;
    let failWorkspace = false;
    const { page } = await fixture(t, { clock: true, beforeOpen: async ({ context, state }) => {
      state.closedTerminalIds.push('wt-1-tab');
      state.allTerminalsGate = new Promise(resolve => { release = resolve; });
      await context.route('**/api/terminals?*', async route => {
        const scope = new URL(route.request().url()).searchParams.get('workspace_root');
        if ((scope === null && failInventory) || (scope === worktrees[1].path && failWorkspace)) {
          await route.fulfill({ status: 503, json: { message: 'Terminal inventory unavailable' } });
        } else await route.fallback();
      });
    } });
    t.after(() => release());
    const row = page.locator(`.project-aow-worktrees button[title="${worktrees[1].path}"]`);
    const isCompact = () => row.evaluate(element => element.classList.contains('compact'));
    assert.equal(await isCompact(), false, 'unknown backend resources must not be treated as empty');
    release();
    await eventually(isCompact);
    failInventory = true;
    await page.clock.runFor(3100);
    await eventually(async () => !await isCompact());
    failInventory = false;
    await page.clock.runFor(3100);
    await eventually(isCompact);
    failWorkspace = true;
    await row.click();
    await surface(page).getByText('Terminal inventory unavailable', { exact: true }).first().waitFor();
    assert.equal(await isCompact(), false, 'a failed local inventory must not reuse an earlier empty snapshot');
    assert.equal(await surface(page).getByRole('tab').count(), 0);
    failWorkspace = false;
    await page.clock.runFor(3100);
    await eventually(isCompact);
  });

  await test('worktree icons combine with colors, sync pinned rows and persist after reload', async t => {
    const { page, state } = await fixture(t);
    const row = page.locator(`.project-aow-worktrees button[title="${worktrees[0].path}"]`);
    const pinned = page.locator(`.project-aow-pinned button[title="${worktrees[0].path}"]`);
    const menu = page.getByRole('menu', { name: `${worktrees[0].path} 操作`, exact: true });
    const openMenu = (target = row) => target.click({ button: 'right' });
    await row.locator('svg.lucide-git-branch').waitFor();
    await row.click({ button: 'right' });
    await menu.getByRole('menuitemradio', { name: '紫色', exact: true }).click();
    await eventually(() => row.locator('svg').evaluate(svg => getComputedStyle(svg).color === 'rgb(168, 85, 247)'));
    await openMenu();
    const rows = [];
    for (const label of ['动物', '水果', '植物']) {
      const group = menu.getByRole('group', { name: label, exact: true });
      const buttons = group.getByRole('menuitemradio');
      assert.equal(await buttons.count(), 10);
      rows.push(await buttons.evaluateAll(items => items.map(item => {
        const rect = item.getBoundingClientRect();
        const svg = item.querySelector('svg');
        return { x: rect.x, y: rect.y, color: getComputedStyle(svg).color, stroke: svg.getAttribute('stroke') };
      })));
    }
    assert.equal(new Set(rows.map(items => items[0].y)).size, 3);
    for (const items of rows) {
      assert.equal(new Set(items.map(item => item.x)).size, 10);
      assert.equal(new Set(items.map(item => item.y)).size, 1);
      assert.ok(items.every(item => item.color === 'rgb(168, 85, 247)' && item.stroke === 'currentColor'));
    }
    await menu.getByRole('menuitemradio', { name: '猫', exact: true }).click();
    await row.locator('svg.lucide-cat').waitFor();
    await row.click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Pin to top', exact: true }).click();
    await pinned.locator('svg.lucide-cat').waitFor();
    await openMenu(pinned);
    assert.equal(await menu.getByRole('menuitemradio', { name: '猫', exact: true }).getAttribute('aria-checked'), 'true');
    if (process.env.WORKTREE_ICON_SCREENSHOT) await page.screenshot({ path: process.env.WORKTREE_ICON_SCREENSHOT });
    await menu.getByRole('menuitemradio', { name: '绿色', exact: true }).click();
    await eventually(() => pinned.locator('svg.lucide-cat').evaluate(svg => getComputedStyle(svg).color === 'rgb(34, 197, 94)'));
    await page.reload();
    await pinned.locator('svg.lucide-cat').waitFor();
    assert.equal(await row.locator('svg.lucide-cat').evaluate(svg => getComputedStyle(svg).color), 'rgb(34, 197, 94)');
    await openMenu();
    await menu.getByRole('menuitemradio', { name: '梨', exact: true }).click();
    await pinned.locator('svg.lucide-pear').waitFor();
    assert.equal(await pinned.locator('svg').evaluate(svg => getComputedStyle(svg).color), 'rgb(34, 197, 94)');
    await openMenu();
    await menu.getByRole('menuitemradio', { name: '默认图标', exact: true }).click();
    await pinned.locator('svg.lucide-git-branch').waitFor();
    assert.equal(await row.locator('svg').evaluate(svg => getComputedStyle(svg).color), 'rgb(34, 197, 94)');
    assert.equal(await page.locator(`.project-aow-worktrees button[title="${worktrees[1].path}"] svg`).getAttribute('class'), 'lucide lucide-git-branch');
    assert.deepEqual(state.appearanceWrites, [
      { path: worktrees[0].path, color: 'purple' }, { path: worktrees[0].path, icon: 'cat' },
      { path: worktrees[0].path, color: 'green' }, { path: worktrees[0].path, icon: 'pear' }, { path: worktrees[0].path, icon: 'default' },
    ]);
    // Opening near the screen edge must position the entire picker inside the viewport.
    await row.evaluate(element => element.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 1438, clientY: 898, button: 2 })));
    const bounds = await menu.boundingBox();
    assert.ok(bounds.x >= 4 && bounds.y >= 4 && bounds.x + bounds.width <= 1436 && bounds.y + bounds.height <= 896, JSON.stringify(bounds));
  });

  await test('worktree icon save failures restore the previous shape and keep its color', async t => {
    const { page, state } = await fixture(t);
    const row = page.locator(`.project-aow-worktrees button[title="${worktrees[0].path}"]`);
    const menu = page.getByRole('menu', { name: `${worktrees[0].path} 操作`, exact: true });
    await row.click({ button: 'right' });
    await menu.getByRole('menuitemradio', { name: '猫', exact: true }).click();
    await eventually(() => state.projects[0].worktrees[0].icon === 'cat');
    let release;
    state.appearanceGate = new Promise(resolve => { release = resolve; });
    state.failAppearance = true;
    await row.click({ button: 'right' });
    await menu.getByRole('menuitemradio', { name: '狗', exact: true }).click();
    await row.locator('svg.lucide-dog').waitFor();
    await row.click({ button: 'right' });
    assert.equal(await menu.getByRole('menuitemradio', { name: '兔子', exact: true }).isDisabled(), true);
    release();
    await row.locator('svg.lucide-cat').waitFor();
    await page.getByText('Appearance save failed', { exact: true }).waitFor();
    assert.equal(await menu.getByRole('menuitemradio', { name: '猫', exact: true }).getAttribute('aria-checked'), 'true');
    assert.equal(await row.locator('svg').evaluate(svg => getComputedStyle(svg).color), 'rgb(121, 169, 235)');
    await page.reload();
    await row.locator('svg.lucide-cat').waitFor();
  });

  await test('terminal polling preserves pane dragging while applying metadata and layout changes', async t => {
    const { page, state } = await fixture(t, { splitTerminal: true });
    let current = structuredClone(state.knownTerminalTabs['wt-0-tab']);
    const originalLayout = structuredClone(current.layout);
    const saves = [];
    await page.route('**/api/terminals?**', async route => {
      if (new URL(route.request().url()).searchParams.get('workspace_root') !== worktrees[0].path) {
        await route.fallback();
        return;
      }
      await route.fulfill({ json: [current] });
    });
    await page.route('**/api/terminals/wt-0-tab/layout', async route => {
      const update = route.request().postDataJSON();
      saves.push(update);
      current = { ...current, layout: update.layout, revision: current.revision + 1 };
      await route.fulfill({ json: current });
    });
    const panes = surface(page).locator('.terminal-pane');
    const source = await panes.nth(0).locator('.terminal-pane-header').boundingBox();
    const target = await panes.nth(1).boundingBox();
    await page.mouse.move(source.x + 25, source.y + source.height / 2);
    await page.mouse.down();
    try {
      await page.mouse.move(source.x + 50, source.y + source.height / 2, { steps: 5 });
      await page.mouse.move(target.x + 30, target.y + target.height / 2, { steps: 10 });
      await page.mouse.move(target.x + 32, target.y + target.height / 2);
      assert.equal(await surface(page).locator('.pane-dragging').count(), 1);
      assert.equal(await surface(page).locator('.pane-drop-target').count(), 1);

      current = { ...current, name: 'Refreshed shell', name_is_custom: true };
      await eventually(async () => await tab(page, 'Refreshed shell').count() === 1);
      await page.mouse.move(target.x + 40, target.y + target.height / 2);
      await page.mouse.move(target.x + 42, target.y + target.height / 2);
      assert.equal(await surface(page).locator('.pane-dragging').count(), 1);
      assert.equal(await surface(page).locator('.pane-drop-target').count(), 1);
    } finally {
      await page.mouse.up();
    }
    await eventually(() => Promise.resolve(saves.length === 1));
    assert.equal(saves[0].revision, 1);
    assert.notDeepEqual(saves[0].layout, originalLayout);

    current = { ...current, revision: current.revision + 1, layout: { ...current.layout, ratio: .65 } };
    await eventually(async () => await surface(page).locator('.terminal-split-child').first()
      .evaluate(element => element.style.flexBasis === '65%'));
    assert.equal(await panes.count(), 3);
  });

  await test('left sidebar hides without remounting workspaces and restores its width and visibility', async t => {
    const { page, state } = await fixture(t);
    const sidebar = page.getByRole('complementary', { name: '项目侧边栏', includeHidden: true });
    assert.equal(await sidebar.locator('.project-aow-brand strong').textContent(), 'AoW');
    const terminal = await surface(page).locator('.terminal-emulator-shell:visible').elementHandle();
    const resizer = page.getByRole('separator', { name: '调整项目栏宽度' });
    const handle = await resizer.boundingBox();
    await page.mouse.move(handle.x + handle.width / 2, handle.y + 100);
    await page.mouse.down();
    await page.mouse.move(handle.x + handle.width / 2 + 80, handle.y + 100);
    await page.mouse.up();
    const original = await sidebar.boundingBox();
    const center = await surface(page).locator('.project-aow-center').boundingBox();
    await page.getByRole('button', { name: '隐藏左侧栏', exact: true }).click();
    assert.equal(await sidebar.isVisible(), false);
    assert.equal(await resizer.isVisible(), false);
    const expanded = await surface(page).locator('.project-aow-center').boundingBox();
    assert.equal(expanded.x, 0);
    assert.equal(expanded.width, center.width + original.width);
    assert.equal(await terminal.evaluate(element => element.isConnected && element.getBoundingClientRect().height > 0), true);
    assert.equal(state.createdTerminals.length, 0);
    assert.deepEqual(state.closedTerminalIds, []);

    const show = page.getByRole('button', { name: '显示左侧栏', exact: true });
    await show.focus();
    await page.keyboard.press('Enter');
    assert.equal((await sidebar.boundingBox()).width, original.width);
    assert.equal(await resizer.isVisible(), true);
    await switchWorktree(page, 1);
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    await page.getByRole('button', { name: '隐藏左侧栏', exact: true }).click();
    await page.reload();
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    assert.equal(await sidebar.isVisible(), false, 'hidden preference survives reload and worktree changes');
    await show.click();
    assert.equal((await sidebar.boundingBox()).width, original.width);
    assert.equal(await page.locator('.project-aow-worktrees button.active').getAttribute('title'), worktrees[1].path);
    await page.reload();
    await sidebar.waitFor({ state: 'visible' });
    assert.equal((await sidebar.boundingBox()).width, original.width);
    if (process.env.SIDEBAR_SCREENSHOT) await page.screenshot({ path: process.env.SIDEBAR_SCREENSHOT });
  });

  await test('CLI terminals stay hidden, cannot be renamed, enable takeover after startup, and hide without terminating', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 20, floating: true });
    const assertNoRename = async selectedTab => {
      await selectedTab.dblclick();
      assert.equal(await page.locator('.project-aow-tab-rename-input').count(), 0);
      await selectedTab.click({ button: 'right' });
      assert.equal(await page.getByRole('menuitem', { name: '重命名', exact: true }).count(), 0);
      await page.keyboard.press('Escape');
      assert.deepEqual(state.names, {});
    };
    assert.equal(await surface(page).getByRole('tab').count(), 1);
    assert.equal(state.terminalMessages.some(item => item.url.includes('/terminals/cli-')), false);
    await surface(page).getByRole('button', { name: 'Terminal 面板', exact: true }).click();
    const list = surface(page).locator('.terminal-panel');
    assert.equal(await list.getByRole('region', { name: 'CLI Terminals' }).locator('.terminal-panel-row').count(), 20);
    const cliRow = list.locator('.terminal-panel-row').filter({ hasText: 'CLI agent 0' });
    assert.equal(await cliRow.locator('.terminal-panel-status-label').textContent(), '初始化中');
    await cliRow.getByRole('img', { name: '已隐藏', exact: true }).waitFor();
    await cliRow.getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.equal(await cliRow.locator('.terminal-panel-status > svg').count(), 2);
    await list.locator('.terminal-panel-row').filter({ hasText: 'Shell' }).getByRole('img', { name: '已接管', exact: true }).waitFor();
    await list.locator('.terminal-panel-open').filter({ hasText: 'CLI agent 0' }).click();
    await surface(page).locator('.terminal-connection.observing').waitFor();
    await cliRow.getByRole('img', { name: '已显示', exact: true }).waitFor();
    await cliRow.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    assert.equal(await surface(page).getByRole('tab').count(), 2);
    const takeover = surface(page).getByRole('button', { name: '接管', exact: true });
    assert.equal(await takeover.isDisabled(), true);
    await assertNoRename(tab(page, 'CLI agent 0'));
    await page.setViewportSize({ width: 1000, height: 750 });
    assert.equal(state.terminalMessages.some(item => item.url.includes('/terminals/cli-') && typeof item.message === 'string' && JSON.parse(item.message).type === 'resize'), false);
    state.createdTerminalTabs[0].panes[0].agent_terminal.phase = 'ready';
    await list.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await eventually(async () => !await takeover.isDisabled());
    assert.equal(await cliRow.locator('.terminal-panel-status-label').textContent(), '运行中');
    await cliRow.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    await takeover.click();
    await surface(page).locator('.terminal-connection.connected').last().waitFor({ state: 'attached' });
    await cliRow.getByRole('img', { name: '已接管', exact: true }).waitFor();
    const controller = state.terminalSockets.findLast(socket => socket.url().includes('/terminals/cli-0/'));
    controller.send(JSON.stringify({ type: 'error', code: 'attachment_superseded', message: '控制权已被其他窗口接管' }));
    await surface(page).locator('.terminal-connection.observing').waitFor();
    await cliRow.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    assert.equal(await cliRow.locator('.terminal-panel-status-label').textContent(), '运行中');
    await takeover.click();
    await cliRow.getByRole('img', { name: '已接管', exact: true }).waitFor();
    await tab(page, 'Shell').click();
    await cliRow.getByRole('img', { name: '已显示', exact: true }).waitFor();
    await cliRow.getByRole('img', { name: '已接管', exact: true }).waitFor();
    await assertNoRename(tab(page, 'CLI agent 0'));
    await tab(page, 'CLI agent 0').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const floatingTab = floating.getByRole('tab').filter({ hasText: 'CLI agent 0' });
    await floating.locator('.terminal-connection.observing').waitFor();
    await cliRow.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    await cliRow.getByRole('img', { name: '已显示', exact: true }).waitFor();
    await floating.getByRole('button', { name: '接管', exact: true }).click();
    await cliRow.getByRole('img', { name: '已接管', exact: true }).waitFor();
    await assertNoRename(floatingTab);
    await floatingTab.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '还原', exact: true }).click();
    await floating.getByRole('button', { name: '最小化浮动工作区' }).click();
    await surface(page).getByRole('button', { name: '关闭 CLI agent 0', exact: true }).click();
    await cliRow.getByRole('img', { name: '已隐藏', exact: true }).waitFor();
    await cliRow.getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.equal(await cliRow.locator('.terminal-panel-status-label').textContent(), '运行中');
    assert.equal(await surface(page).getByRole('tab').count(), 1);
    assert.deepEqual(state.closedTerminalIds, []);
    await list.locator('.terminal-panel-open').filter({ hasText: 'CLI agent 0' }).click();
    await surface(page).locator('.terminal-connection.observing').waitFor();
    await cliRow.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    await list.locator('.terminal-panel-open').filter({ hasText: 'CLI agent 0' }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: '销毁', exact: true }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: '销毁', exact: true }).click();
    await eventually(() => Promise.resolve(state.closedTerminalIds.includes('cli-0')));
  });

  await test('terminal list shows open tabs independently of their connection and active selection', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1 });
    let release;
    state.terminalClaimGate = new Promise(resolve => { release = resolve; });
    t.after(() => release());
    const row = surface(page).locator('.terminal-panel-row').filter({ hasText: 'CLI agent 0' });
    await row.locator('.terminal-panel-open').click();
    await row.getByRole('img', { name: '已显示', exact: true }).waitFor();
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.equal(await row.getByRole('img', { name: '旁观中', exact: true }).count(), 0);
    release();
    await row.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    await tab(page, 'Shell').click();
    assert.deepEqual(await row.locator('.terminal-panel-status > svg').evaluateAll(icons => icons.map(icon => icon.getAttribute('aria-label'))), ['已显示', '旁观中']);
    await surface(page).getByRole('button', { name: '关闭 CLI agent 0', exact: true }).click();
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.deepEqual(await row.locator('.terminal-panel-status > svg').evaluateAll(icons => icons.map(icon => icon.getAttribute('aria-label'))), ['已隐藏', '未连接']);
    assert.deepEqual(state.closedTerminalIds, []);
  });

  await test('terminal state aggregation is independent of pane order and connection progress', async () => {
    const { terminalTabState, terminalPaneStatusMessage } = await server.ssrLoadModule('/src/features/terminals/terminalState.ts');
    const pane = (id, status, phase) => ({ id, status, ...(phase && { agent_terminal: { phase } }) });
    const cases = [
      [[], 'closed'],
      [[pane('a', 'exited'), pane('b', 'running')], 'running'],
      [[pane('a', 'interrupted'), pane('b', 'running', 'starting')], 'starting'],
      [[pane('a', 'exited'), pane('b', 'running', 'failed')], 'failed'],
      [[pane('a', 'running', 'failed'), pane('b', 'running', 'starting')], 'starting'],
      [[pane('a', 'running', 'failed'), pane('b', 'running', 'ready')], 'running'],
      [[pane('a', 'exited', 'failed'), pane('b', 'interrupted', 'starting')], 'interrupted'],
      [[pane('a', 'exited', 'starting'), pane('b', 'exited', 'failed')], 'exited'],
    ];
    for (const [panes, lifecycle] of cases) {
      for (const ordered of [panes, [...panes].reverse()]) {
        assert.equal(terminalTabState({ panes: ordered }, false, () => undefined).lifecycle, lifecycle);
      }
    }
    const tab = { panes: [pane('finished', 'exited', 'failed'), pane('live', 'running', 'ready')] };
    const connections = new Map([['finished', 'connected'], ['live', 'observing']]);
    assert.deepEqual(terminalTabState(tab, false, id => connections.get(id)), { lifecycle: 'running', display: 'shown', control: 'observing' });
    connections.set('live', 'connected');
    assert.equal(terminalTabState(tab, false, id => connections.get(id)).control, 'controlled');
    for (const progress of ['connecting', 'waiting', 'disconnected']) {
      connections.set('live', progress);
      assert.equal(terminalTabState(tab, true, id => connections.get(id)).control, 'disconnected');
    }
    assert.deepEqual(terminalTabState(tab, true, () => undefined), { lifecycle: 'running', display: 'shown', control: 'disconnected' });
    assert.deepEqual(terminalTabState(tab, false, () => undefined), { lifecycle: 'running', display: 'hidden', control: 'disconnected' });
    assert.equal(terminalPaneStatusMessage(pane('a', 'running', 'starting'), 'exited'), '已退出');
    assert.equal(terminalPaneStatusMessage(pane('a', 'interrupted', 'failed'), 'observing'), '已中断');
  });

  await test('terminal list aggregates running and finished panes across the whole tab', async t => {
    const { page, state } = await fixture(t, { splitTerminal: true });
    const current = structuredClone(state.knownTerminalTabs['wt-0-tab']);
    current.panes[0].status = 'exited';
    await page.route('**/api/terminals?**', async route => {
      if (new URL(route.request().url()).searchParams.get('workspace_root') !== worktrees[0].path) return route.fallback();
      await route.fulfill({ json: [current] });
    });
    const list = surface(page).locator('.terminal-panel');
    const row = list.locator('.terminal-panel-row').filter({ hasText: 'Shell' });
    const refresh = () => list.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await refresh();
    await surface(page).locator('.terminal-pane-status.exited').waitFor();
    assert.equal(await row.locator('.terminal-panel-status-label').textContent(), '运行中');
    await row.getByRole('img', { name: '已接管', exact: true }).waitFor();
    current.panes[1].status = 'interrupted';
    current.panes[2].status = 'exited';
    await refresh();
    await eventually(async () => await row.locator('.terminal-panel-status-label').textContent() === '已中断');
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    current.panes[1].status = 'exited';
    await refresh();
    await eventually(async () => await row.locator('.terminal-panel-status-label').textContent() === '已退出');
  });

  await test('terminal startup banners retain actual connection errors instead of claiming observation', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1 });
    const row = surface(page).locator('.terminal-panel-row').filter({ hasText: 'CLI agent 0' });
    await row.locator('.terminal-panel-open').click();
    await row.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    assert.equal(await surface(page).locator('.terminal-connection.observing > span').textContent(), '初始化中 · 旁观中');
    assert.equal(await surface(page).locator('.terminal-pane-status.starting').evaluate(el => getComputedStyle(el).backgroundColor),
      await row.locator('.terminal-panel-status-dot').evaluate(el => getComputedStyle(el).backgroundColor));
    const socket = state.terminalSockets.findLast(socket => socket.url().includes('/terminals/cli-0/'));
    socket.send(JSON.stringify({ type: 'control', state: 'invalid' }));
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    const banner = surface(page).locator('.terminal-connection.disconnected');
    await banner.waitFor();
    assert.equal(await banner.locator('span').textContent(), '初始化中 · 服务器返回了无效的终端控制状态');
    await banner.getByRole('button', { name: '立即重连' }).click();
    await row.getByRole('img', { name: '旁观中', exact: true }).waitFor();
  });

  for (const status of ['exited', 'interrupted']) await test(`rebuild ${status} CLI terminal keeps its tab and replaces the attached pane`, async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1, beforeOpen: ({ state }) => {
      state.createdTerminalTabs[0].panes[0].status = status;
      state.createdTerminalTabs[0].panes[0].agent_terminal.phase = 'ready';
    } });
    const list = surface(page).locator('.terminal-panel');
    const row = list.locator('.terminal-panel-row').filter({ hasText: 'CLI agent 0' });
    await row.locator('.terminal-panel-open').click();
    const oldPane = await surface(page).locator('.terminal-pane:visible .xterm').elementHandle();
    let rebuilds = 0;
    let release;
    const gate = new Promise(resolve => { release = resolve; });
    await page.route('**/api/terminals/cli-0/rebuild', async route => {
      assert.equal(route.request().method(), 'POST');
      rebuilds += 1;
      await gate;
      const rebuilt = state.createdTerminalTabs[0];
      rebuilt.revision += 1;
      rebuilt.layout.pane_id = 'rebuilt-pane';
      rebuilt.panes[0] = { ...rebuilt.panes[0], id: 'rebuilt-pane', status: 'running', exit_code: null,
        agent_terminal: { phase: 'starting', error: null, task_submitted: false } };
      state.knownTerminalTabs[rebuilt.id] = rebuilt;
      await route.fulfill({ json: rebuilt });
    });
    await row.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '重建', exact: true }).click();
    await eventually(() => rebuilds === 1);
    await row.click({ button: 'right' });
    assert.equal(await page.getByRole('menuitem', { name: '重建中…', exact: true }).isDisabled(), true);
    await page.keyboard.press('Escape');
    release();
    await row.getByText('初始化中', { exact: true }).waitFor();
    await eventually(() => state.terminalSockets.some(socket => socket.url().includes('/panes/rebuilt-pane/')));
    assert.equal(await oldPane.evaluate(node => node.isConnected), false);
    assert.equal(await surface(page).getByRole('tab', { name: /CLI agent 0/ }).count(), 1);
    assert.equal(await row.count(), 1);
    assert.equal(rebuilds, 1);
    assert.deepEqual(state.closedTerminalIds, []);
    await row.click({ button: 'right' });
    assert.equal(await page.getByRole('menuitem', { name: '重建', exact: true }).count(), 0);
  });

  await test('failed rebuild leaves interrupted terminal available for retry', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1, beforeOpen: ({ state }) => {
      state.createdTerminalTabs[0].panes[0].status = 'interrupted';
    } });
    const row = surface(page).locator('.terminal-panel-row').filter({ hasText: 'CLI agent 0' });
    await page.route('**/api/terminals/cli-0/rebuild', route => route.fulfill({ status: 503, json: { message: '重建失败：服务暂不可用' } }));
    await row.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '重建', exact: true }).click();
    await surface(page).getByRole('alert').filter({ hasText: '重建失败：服务暂不可用' }).waitFor();
    await row.getByText('已中断', { exact: true }).waitFor();
    assert.equal(state.createdTerminalTabs[0].panes[0].id, 'cli-pane-0');
    await row.click({ button: 'right' });
    assert.equal(await page.getByRole('menuitem', { name: '重建', exact: true }).isEnabled(), true);
  });

  await test('terminal list lifecycle stays independent of hidden, observer and exited views', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1 });
    const list = surface(page).locator('.terminal-panel');
    const row = list.locator('.terminal-panel-row').filter({ hasText: 'CLI agent 0' });
    state.createdTerminalTabs[0].panes[0].agent_terminal.phase = 'failed';
    await list.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await eventually(async () => await row.locator('.terminal-panel-status-label').textContent() === '启动失败');
    await row.getByRole('img', { name: '已隐藏', exact: true }).waitFor();
    await row.locator('.terminal-panel-open').click();
    await row.getByRole('img', { name: '旁观中', exact: true }).waitFor();
    assert.equal(await surface(page).locator('.terminal-connection.observing > span').textContent(), '启动失败 · 旁观中 · 请查看终端输出');
    assert.equal(await surface(page).locator('.terminal-pane-status.failed').evaluate(el => getComputedStyle(el).backgroundColor),
      await row.locator('.terminal-panel-status-dot').evaluate(el => getComputedStyle(el).backgroundColor));
    state.createdTerminalTabs[0].panes[0].status = 'exited';
    await list.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await eventually(async () => await row.locator('.terminal-panel-status-label').textContent() === '已退出');
    await row.getByRole('img', { name: '已显示', exact: true }).waitFor();
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    await surface(page).locator('.terminal-connection.exited').waitFor();
    assert.equal(await surface(page).locator('.terminal-connection.exited > span').textContent(), '已退出');
    assert.equal(await row.locator('.terminal-panel-status > svg').count(), 2);
    await surface(page).getByRole('button', { name: '关闭 CLI agent 0', exact: true }).click();
    await row.getByRole('img', { name: '已隐藏', exact: true }).waitFor();
    await row.getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.equal(await row.locator('.terminal-panel-status-label').textContent(), '已退出');
    state.createdTerminalTabs[0].panes[0].status = 'interrupted';
    await list.getByRole('button', { name: '刷新 CLI Terminals', exact: true }).click();
    await eventually(async () => await row.locator('.terminal-panel-status-label').textContent() === '已中断');
    assert.deepEqual(state.closedTerminalIds, []);
  });

  await test('repository terminal scope persists independent list choices with main first and no extra terminal connections', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1 });
    const list = surface(page).locator('.terminal-panel');
    const users = list.getByRole('region', { name: 'User Terminals', exact: true });
    const agents = list.getByRole('region', { name: 'CLI Terminals', exact: true });
    state.createdTerminalTabs.push({ ...structuredClone(state.createdTerminalTabs[0]), id: 'foreign-cli', name: 'Worktree agent', workspace_root: worktrees[1].path });
    state.createdTerminalTabs.push({ ...structuredClone(state.createdTerminalTabs[0]), id: 'unrelated-cli', name: 'Unrelated agent', workspace_root: '/another-repository' });
    state.projects[0] = { ...project, worktrees: [worktrees[1], worktrees[0], { ...worktrees[1], id: 'empty', path: '/workspace/empty' }] };
    await page.getByRole('button', { name: '刷新全部', exact: true }).click();
    await eventually(() => state.terminalListScopes.filter(scope => scope === null).length >= 2);
    assert.equal(await list.locator('.terminal-worktree-heading').count(), 0);
    const connectionCount = state.terminalMessages.filter(item => typeof item.message === 'string' && JSON.parse(item.message).type === 'claim').length;
    await users.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    const option = page.getByRole('menuitemcheckbox', { name: '显示所有Worktree', exact: true });
    assert.equal(await option.getAttribute('aria-checked'), 'false');
    await option.click();
    await eventually(async () => await users.locator('.terminal-worktree-heading').count() === 2);
    assert.equal(await agents.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await agents.locator('.terminal-panel-row').count(), 1, 'enabling user terminals keeps CLI terminals local');
    await agents.getByRole('button', { name: 'CLI Terminals 显示范围', exact: true }).click();
    assert.equal(await option.getAttribute('aria-checked'), 'false');
    await option.click();
    await eventually(async () => await list.locator('.terminal-worktree-heading').count() === 4);
    for (const group of [users, agents]) {
      assert.deepEqual(await group.locator('.terminal-worktree-heading > span').allTextContents(), ['主仓库 · branch-0', 'branch-1']);
      assert.deepEqual(await group.locator('.terminal-worktree-heading .terminal-panel-count').allTextContents(), ['1', '1']);
      assert.equal(await group.locator('.aow-panel-header .terminal-panel-count').textContent(), '2');
      assert.match(await group.locator('.terminal-worktree-heading').first().getAttribute('title'), /\/workspace\/wt-0/);
    }
    assert.equal(await list.getByText('Unrelated agent', { exact: true }).count(), 0);
    assert.equal(await list.getByRole('region', { name: '/workspace/empty', exact: true }).count(), 0);
    assert.equal(await surface(page).getByRole('tab').count(), 1);
    assert.equal(state.terminalMessages.filter(item => typeof item.message === 'string' && JSON.parse(item.message).type === 'claim').length, connectionCount);
    const foreignGroup = users.getByRole('region', { name: worktrees[1].path, exact: true });
    await foreignGroup.locator('.terminal-worktree-heading').click();
    assert.equal(await foreignGroup.locator('.terminal-panel-row').isVisible(), false);
    await foreignGroup.locator('.terminal-worktree-heading').press('Enter');
    assert.equal(await foreignGroup.locator('.terminal-panel-row').isVisible(), true);
    await agents.getByRole('button', { name: 'CLI Terminals 显示范围', exact: true }).click();
    assert.equal(await option.getAttribute('aria-checked'), 'true');
    await page.keyboard.press('Escape');
    assert.equal(await agents.getByRole('button', { name: 'CLI Terminals 显示范围', exact: true }).evaluate(node => node === document.activeElement), true);
    if (process.env.TERMINAL_SCOPE_SCREENSHOT) await page.screenshot({ path: process.env.TERMINAL_SCOPE_SCREENSHOT });
    await users.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await option.click();
    assert.equal(await users.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await users.locator('.terminal-panel-row').count(), 1);
    assert.equal(await agents.locator('.terminal-worktree-heading').count(), 2, 'disabling user terminals keeps CLI terminals global');
    await page.reload();
    await eventually(async () => await agents.locator('.terminal-worktree-heading').count() === 2);
    assert.equal(await users.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await users.locator('.terminal-panel-row').count(), 1);
    await users.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    assert.equal(await option.getAttribute('aria-checked'), 'false', 'the user terminal choice survives reload');
    await page.keyboard.press('Escape');
    await switchWorktree(page, 1);
    assert.equal(await surface(page).getByRole('button', { name: /Terminals 显示范围/ }).count(), 0);
    assert.equal(await surface(page).locator('.terminal-worktree-heading').count(), 0);
    await switchWorktree(page, 0);
    await users.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await option.click();
    await agents.getByRole('button', { name: 'CLI Terminals 显示范围', exact: true }).click();
    assert.equal(await option.getAttribute('aria-checked'), 'true', 'the CLI terminal choice survives reload and worktree switches');
    await option.click();
    assert.equal(await users.locator('.terminal-worktree-heading').count(), 2, 'disabling CLI terminals keeps user terminals global');
    assert.equal(await agents.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await agents.locator('.terminal-panel-row').count(), 1);
    await page.reload();
    await eventually(async () => await users.locator('.terminal-worktree-heading').count() === 2);
    assert.equal(await agents.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await agents.locator('.terminal-panel-row').count(), 1);
    await agents.getByRole('button', { name: 'CLI Terminals 显示范围', exact: true }).click();
    assert.equal(await option.getAttribute('aria-checked'), 'false');
    await page.keyboard.press('Escape');
    await users.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await option.click();
    assert.equal(await list.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await users.locator('.terminal-panel-row').count(), 1);
    assert.equal(await agents.locator('.terminal-panel-row').count(), 1);
    const globalAgentRequests = state.terminalAgentScopes.filter(scope => scope === null).length;
    await delay(3200);
    assert.equal(state.terminalAgentScopes.filter(scope => scope === null).length, globalAgentRequests, 'sidebar inventory must not enable global terminal agent polling');
  });

  await test('repository terminal scope opens existing worktree tabs and confirms cross-worktree destruction', async t => {
    const { page, state } = await fixture(t);
    state.names['wt-1-tab'] = 'Remote shell';
    state.agents['wt-1-pane'] = 'codex';
    const list = surface(page).locator('.terminal-panel');
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    const remote = () => list.locator('.terminal-panel-open').filter({ hasText: 'Remote shell' });
    await remote().waitFor();
    await remote().getByRole('img', { name: '已隐藏', exact: true }).waitFor();
    await remote().getByRole('img', { name: '未连接', exact: true }).waitFor();
    assert.ok((await remote().locator('img.agent-icon').getAttribute('src')).endsWith('codex.png'));
    await remote().click();
    await eventually(async () => await page.locator(`.project-aow-worktrees button[title="${worktrees[0].path}"]`).getAttribute('class') === 'active');
    await surface(page).getByRole('tab').filter({ hasText: 'Remote shell' }).waitFor();
    assert.equal(state.createdTerminals.length, 0);
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    await remote().getByRole('img', { name: '已接管', exact: true }).waitFor();
    await remote().getByRole('img', { name: '已显示', exact: true }).waitFor();
    assert.equal(await surface(page).getByRole('button', { name: /Terminals 显示范围/ }).count(), 2);
    await remote().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await floating.getByRole('tab').filter({ hasText: 'Remote shell' }).waitFor();
    await floating.locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    await remote().getByRole('img', { name: '已接管', exact: true }).waitFor();
    await remote().getByRole('img', { name: '已显示', exact: true }).waitFor();
    assert.equal(await surface(page).getByRole('tab').filter({ hasText: 'Remote shell' }).count(), 0, 'moving to floating removes the main repository tab');
    await floating.getByRole('button', { name: '最小化浮动工作区' }).click();
    await switchWorktree(page, 0);
    await remote().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '销毁', exact: true }).click();
    const confirmation = page.getByRole('alertdialog', { name: '销毁终端？', exact: true });
    await confirmation.getByRole('button', { name: '取消', exact: true }).click();
    assert.deepEqual(state.closedTerminalIds, []);
    await remote().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '销毁', exact: true }).click();
    await confirmation.getByRole('button', { name: '销毁', exact: true }).click();
    await remote().waitFor({ state: 'detached' });
    assert.deepEqual(state.closedTerminalIds, ['wt-1-tab']);
    assert.equal(state.createdTerminals.length, 0);
    assert.equal(await list.locator('.terminal-worktree-heading').count(), 1, 'empty worktree groups disappear');
    await eventually(async () => await page.locator('.floating-workspace-host .terminal-emulator-shell').count() === 0);
  });

  await test('repository terminal hosting keeps one view, restores from either workspace, persists and closes without destroying', async t => {
    const { page, state } = await fixture(t);
    state.names['wt-1-tab'] = 'Remote shell';
    const list = surface(page).locator('.terminal-panel');
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    const openRemote = () => list.locator('.terminal-panel-open').filter({ hasText: 'Remote shell' }).click();
    const ready = () => surface(page).locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    await openRemote();
    await tab(page, 'Remote shell').waitFor();
    await ready();
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), worktrees[0].path);
    await openRemote();
    assert.equal(await tab(page, 'Remote shell').count(), 1);
    assert.equal(await page.locator('.terminal-emulator-shell').count(), 2, 'the main and remote shells each have one renderer');
    await switchWorktree(page, 1);
    await surface(page).getByText('此 Tab 正在主仓库工作区显示。').waitFor();
    assert.equal(await surface(page).locator('.terminal-emulator-shell:visible').count(), 0);
    await surface(page).getByRole('button', { name: '移回当前工作区', exact: true }).click();
    await ready();
    await switchWorktree(page, 0);
    assert.equal(await tab(page, 'Remote shell').count(), 0);
    await openRemote();
    await ready();
    await tab(page, 'Remote shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '还原', exact: true }).click();
    await tab(page, 'Remote shell').waitFor({ state: 'detached' });
    await switchWorktree(page, 1);
    await ready();
    await switchWorktree(page, 0);
    await openRemote();
    await ready();
    await page.reload();
    await tab(page, 'Remote shell').waitFor();
    await ready();
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), worktrees[0].path);
    assert.match(page.url(), /\/aow\/tabs\/terminal\/wt-1-tab/);
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    await ready();
    assert.equal(await tab(page, 'Remote shell').count(), 1, 'changing the listing scope preserves the opened terminal');
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    await tab(page, 'Remote shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '重命名', exact: true }).click();
    const input = surface(page).getByRole('textbox', { name: 'Terminal 名称', exact: true });
    await input.fill('Hosted renamed');
    await input.press('Enter');
    await tab(page, 'Hosted renamed').waitFor();
    assert.equal(state.names['wt-1-tab'], 'Hosted renamed');
    await surface(page).getByRole('button', { name: '关闭 Hosted renamed', exact: true }).click();
    await tab(page, 'Hosted renamed').waitFor({ state: 'detached' });
    await switchWorktree(page, 1);
    assert.equal(await surface(page).getByRole('tab').count(), 0, 'closing also removes the source placeholder');
    assert.deepEqual(state.closedTerminalIds, []);
    await switchWorktree(page, 0);
    await list.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await list.locator('.terminal-panel-open').filter({ hasText: 'Hosted renamed' }).click();
    await ready();
    assert.equal(state.createdTerminals.length, 0);
    assert.equal(await page.locator('.terminal-emulator-shell').count(), 2);
  });

  async function openHostedSibling(page) {
    const list = surface(page).locator('.terminal-panel');
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    await list.locator('.terminal-panel-open').filter({ hasText: 'Remote shell' }).click();
    await tab(page, 'Remote shell').waitFor();
  }
  const selectLocalTerminal = page => surface(page).locator('[data-workspace-tab-id="terminal:wt-0-tab"]').click();
  const selectRemoteTerminal = page => surface(page).locator('[data-workspace-tab-id="terminal:wt-1-tab"]').click();

  await test('main workspace sidebar Explorer follows hosted and file tabs while preserving panel visibility and collapse state', async t => {
    const { page, state } = await fixture(t);
    state.names['wt-1-tab'] = 'Remote shell';
    await openHostedSibling(page);
    const reads = [];
    page.on('request', request => reads.push(new URL(request.url()).pathname));
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    const explorer = surface(page).locator('.explorer').first();
    const file = root => explorer.locator(`[data-tree-path="${root}/edit.txt"]`);
    await file(worktrees[1].path).waitFor();
    await file(worktrees[1].path).dblclick();
    await surface(page).locator('.editor-toolbar:visible code').filter({ hasText: worktrees[1].path }).waitFor();
    await selectLocalTerminal(page);
    await file(worktrees[0].path).waitFor();
    await tab(page, 'edit.txt').click();
    await file(worktrees[1].path).waitFor();
    assert.equal(await file(worktrees[1].path).getAttribute('class').then(value => value.includes('selected')), true);
    assert.equal(reads.some(path => path === '/api/git/status' || path === '/api/aow/agent-sessions'), false);

    const heading = explorer.getByRole('button', { name: /^(展开|收起) Project Explorer$/ });
    await heading.click();
    await selectLocalTerminal(page);
    assert.equal(await heading.getAttribute('aria-expanded'), 'false');
    await selectRemoteTerminal(page);
    assert.equal(await heading.getAttribute('aria-expanded'), 'false');
    await heading.click();
    await file(worktrees[1].path).waitFor();
    await surface(page).getByRole('button', { name: '隐藏右侧栏', exact: true }).click();
    const hiddenReads = reads.length;
    await selectLocalTerminal(page);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    assert.equal(await surface(page).locator('.project-aow-right').isVisible(), false);
    assert.equal(reads.slice(hiddenReads).some(path => path.startsWith('/api/fs/tree')), false);
    await surface(page).getByRole('button', { name: '显示右侧栏', exact: true }).click();
    await file(worktrees[0].path).waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), worktrees[0].path);
  });

  await test('main workspace sidebar Source Control follows hosted and diff tabs without resetting collapsed sections', async t => {
    const { page, state } = await fixture(t);
    state.names['wt-1-tab'] = 'Remote shell';
    await openHostedSibling(page);
    const repositories = [];
    page.on('request', request => {
      const url = new URL(request.url());
      if (url.pathname === '/api/git/status') repositories.push(url.searchParams.get('repo'));
    });
    await surface(page).getByRole('button', { name: 'Source Control', exact: true }).click();
    const git = surface(page).locator('.source-control');
    await eventually(() => repositories.at(-1) === worktrees[1].path);
    await git.locator('.change-row').first().dblclick();
    await tab(page, 'file-0.txt (Diff)').waitFor();
    assert.equal(state.diffRequests.at(-1).repo, worktrees[1].path);
    const changes = git.getByRole('button', { name: /^(展开|收起) Changes$/ });
    await changes.click();
    await selectLocalTerminal(page);
    await eventually(() => repositories.at(-1) === worktrees[0].path);
    assert.equal(await changes.getAttribute('aria-expanded'), 'false');
    await tab(page, 'file-0.txt (Diff)').click();
    await eventually(() => repositories.at(-1) === worktrees[1].path);
    assert.equal(await changes.getAttribute('aria-expanded'), 'false');
    await changes.click();
    await git.locator('.change-row.selected').waitFor();
    await surface(page).getByRole('button', { name: '关闭 file-0.txt (Diff)', exact: true }).click();
    await surface(page).getByRole('button', { name: '关闭 Remote shell', exact: true }).click();
    await eventually(() => repositories.at(-1) === worktrees[0].path);
    assert.equal(await surface(page).getByRole('button', { name: 'Source Control', exact: true }).getAttribute('class'), 'active');
  });

  await test('main workspace sidebar Conversation follows tabs, keeps collapse state and ignores obsolete requests', async t => {
    const { page, state } = await fixture(t, { registeredAgents: [{ id: 'codex', display_name: 'Codex', available: true, args: [], env: {} }] });
    state.names['wt-1-tab'] = 'Remote shell';
    await openHostedSibling(page);
    await selectLocalTerminal(page);
    const sessionFor = root => ({ id: `codex:${root}`, session_id: root.split('/').at(-1), agent: 'codex', title: `Session ${root}`, cwd: `${root}/src`, created_at: task.created_at, updated_at: task.updated_at });
    const reads = [];
    let release;
    const gate = new Promise(resolve => { release = resolve; });
    t.after(release);
    let delayRemote = true;
    await page.context().route('**/api/aow/agent-sessions?**', async route => {
      const root = new URL(route.request().url()).searchParams.get('worktree_path');
      reads.push(root);
      if (root === worktrees[1].path && delayRemote) await gate;
      await route.fulfill({ json: [sessionFor(root)] });
    });
    const snapshotRoots = [];
    await page.context().route('**/api/aow/agent-sessions/*/snapshot?**', async route => {
      const root = new URL(route.request().url()).searchParams.get('worktree_path');
      snapshotRoots.push(root);
      await route.fulfill({ json: { ...sessionFor(root), turns: [], captured_at: task.updated_at, status: 'completed', truncated: false } });
    });
    await surface(page).getByRole('button', { name: 'Conversation', exact: true }).click();
    const sessions = surface(page).locator('.project-aow-sessions');
    const row = root => sessions.locator('.project-aow-session-row').filter({ hasText: `Session ${root}` });
    await row(worktrees[0].path).waitFor();
    await sessions.getByRole('button', { name: /^(展开|收起) Codex$/ }).click();
    await selectRemoteTerminal(page);
    await eventually(() => reads.at(-1) === worktrees[1].path);
    assert.equal(await sessions.getByRole('button', { name: /^(展开|收起) Codex$/ }).getAttribute('aria-expanded'), 'false');
    await selectLocalTerminal(page);
    await eventually(() => reads.at(-1) === worktrees[0].path);
    await sessions.getByRole('button', { name: /^(展开|收起) Codex$/ }).click();
    await row(worktrees[0].path).waitFor();
    delayRemote = false;
    const oldResponse = page.waitForResponse(response => response.url().includes('/agent-sessions?') && new URL(response.url()).searchParams.get('worktree_path') === worktrees[1].path);
    release();
    await oldResponse;
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    await row(worktrees[0].path).waitFor();
    assert.equal(await row(worktrees[1].path).count(), 0);
    await selectRemoteTerminal(page);
    await row(worktrees[1].path).waitFor();
    await row(worktrees[1].path).click();
    await eventually(() => snapshotRoots.length === 1);
    assert.equal(snapshotRoots[0], `${worktrees[1].path}/src`, 'snapshot keeps the authoritative session cwd while the sidebar follows its worktree');
    await selectLocalTerminal(page);
    await row(worktrees[0].path).waitFor();
    await tab(page, `Session ${worktrees[1].path}`).click();
    await row(worktrees[1].path).waitFor();
    assert.match(await row(worktrees[1].path).getAttribute('class'), /active/);
    assert.equal(await surface(page).getByRole('button', { name: 'Conversation', exact: true }).getAttribute('class'), 'active');
    await row(worktrees[1].path).click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Resume · Codex', exact: true }).click();
    await surface(page).locator('[data-workspace-tab-id="terminal:created-1"].active').waitFor();
    assert.equal(state.createdTerminals.at(-1).workspace_root, worktrees[1].path);
    assert.equal(state.createdTerminals.at(-1).cwd, `${worktrees[1].path}/src`);
    assert.equal(state.createdTerminals.at(-1).resume_session_id, 'wt-1');
    await row(worktrees[1].path).waitFor();
  });

  for (const builtin of [false, true]) await test(`sidebar keeps its workspace context in ${builtin ? 'the floating global workspace' : 'a non-main worktree'}`, async t => {
    const root = builtin ? '/global' : worktrees[1].path;
    const foreignRoot = worktrees[0].path;
    const session = { id: 'codex:foreign', session_id: 'foreign', agent: 'codex', title: 'Other workspace session', cwd: foreignRoot, created_at: task.created_at, updated_at: task.updated_at };
    const queries = { git: [], sessions: [] };
    const { page } = await fixture(t, { floating: builtin, registeredAgents: [{ id: 'codex', display_name: 'Codex', available: true, args: [], env: {} }], beforeOpen: async ({ context, state }) => {
      state.sessionFixtures = [session];
      await context.route('**/api/aow/agent-sessions/*/snapshot?**', route => route.fulfill({ json: { ...session, turns: [], captured_at: task.updated_at, status: 'completed', truncated: false } }));
      context.on('request', request => {
        const url = new URL(request.url());
        if (url.pathname === '/api/git/status') queries.git.push(url.searchParams.get('repo'));
        if (url.pathname === '/api/aow/agent-sessions') queries.sessions.push(url.searchParams.get('worktree_path'));
      });
      await context.addInitScript(({ root, foreignRoot, session, builtin }) => {
        const path = `${foreignRoot}/edit.txt`;
        localStorage.setItem(`aow-workspace-tabs:${root}`, JSON.stringify({
          documents: [
            { id: path, path, name: 'edit.txt', explorerSource: 'project' },
            { id: `diff:${foreignRoot}:false:file-0.txt`, path: 'file-0.txt', name: 'file-0.txt (Diff)', kind: 'diff', readOnly: true, diffSource: { kind: 'working', repository: foreignRoot, staged: false } },
          ],
          sessions: [{ session, workspacePath: foreignRoot }],
          active: `document:${path}`,
        }));
        if (builtin) {
          localStorage.setItem('aow-floating-tabs-v1', JSON.stringify([{ workspace: root, id: `document:${path}`, kind: 'file', label: 'edit.txt', targetId: path }]));
          localStorage.setItem('aow-floating-open', 'true');
        }
      }, { root, foreignRoot, session, builtin });
    } });
    if (!builtin) await switchWorktree(page, 1);
    const area = builtin ? page.getByRole('dialog', { name: '浮动工作区', exact: true }) : surface(page);
    if (builtin) await area.getByRole('button', { name: '全局项目面板', exact: true }).click();
    const selectedTab = name => area.getByRole('tab').filter({ hasText: name });
    await selectedTab('edit.txt').click();
    await area.getByRole('button', { name: 'Explorer', exact: true }).click();
    const explorer = area.locator('.explorer').first();
    await explorer.locator(`[data-tree-path="${root}"]`).waitFor();
    assert.equal(await explorer.locator(`[data-tree-path="${foreignRoot}"]`).count(), 0);
    await selectedTab('file-0.txt (Diff)').click();
    await area.getByRole('button', { name: 'Source Control', exact: true }).click();
    await area.locator('.change-row').first().waitFor();
    assert.deepEqual([...new Set(queries.git)], [root]);
    await selectedTab(session.title).click();
    await area.getByRole('button', { name: 'Conversation', exact: true }).click();
    await area.locator('.project-aow-session-row').filter({ hasText: session.title }).waitFor();
    await selectedTab('edit.txt').click();
    assert.deepEqual([...new Set(queries.sessions)], [root]);
    assert.equal(await area.getByRole('button', { name: 'Conversation', exact: true }).getAttribute('class'), 'active');
  });

  await test('repository hosted terminals close as a group and transfer from floating without changing worktrees', async t => {
    const { page, state } = await fixture(t, { multipleTerminals: true });
    state.names['wt-1-tab'] = 'Remote shell';
    state.names['wt-1-background'] = 'Remote background';
    const list = surface(page).locator('.terminal-panel');
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    const remote = list.locator('.terminal-panel-open').filter({ hasText: 'Remote shell' });
    await remote.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await floating.locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    await floating.getByRole('button', { name: '最小化浮动工作区' }).click();
    await remote.click();
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    assert.equal(await floating.getByRole('tab').count(), 0);
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), worktrees[0].path);
    await list.locator('.terminal-panel-open').filter({ hasText: 'Remote background' }).click();
    await tab(page, 'Remote background').waitFor();
    await tab(page, 'Remote shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '关闭其他', exact: true }).click();
    await eventually(async () => await surface(page).getByRole('tab').count() === 1);
    await switchWorktree(page, 1);
    assert.equal(await surface(page).getByRole('tab').count(), 1);
    await surface(page).getByText('此 Tab 正在主仓库工作区显示。').waitFor();
    await surface(page).getByRole('button', { name: '关闭 Remote shell', exact: true }).click();
    await switchWorktree(page, 0);
    await eventually(async () => await surface(page).getByRole('tab').count() === 0);
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(state.createdTerminals.length, 0);
  });

  await test('repository hosted terminals discard persisted views when the source terminal has disappeared', async t => {
    const { page, state } = await fixture(t);
    state.names['wt-1-tab'] = 'Remote shell';
    const list = surface(page).locator('.terminal-panel');
    await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    await list.locator('.terminal-panel-open').filter({ hasText: 'Remote shell' }).click();
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    await surface(page).locator('[data-workspace-tab-id="terminal:wt-0-tab"]').click();
    state.closedTerminalIds.push('wt-1-tab');
    await page.reload();
    await eventually(async () => (await page.evaluate(() => JSON.parse(localStorage.getItem('aow-hosted-terminals-v1') ?? '[]'))).length === 0);
    assert.equal(await tab(page, 'Remote shell').count(), 0);
    assert.equal(await page.locator('.terminal-emulator-shell').count(), 1);
    assert.equal(state.createdTerminals.length, 0);
  });

  await test('repository terminal scope can open a new CLI tab before its worktree poll discovers it', async t => {
    const { page, state } = await fixture(t, { cliTerminals: 1 });
    await switchWorktree(page, 1);
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    await switchWorktree(page, 0);
    const staleTab = structuredClone(state.knownTerminalTabs['wt-1-tab']);
    const staleList = async route => {
      if (new URL(route.request().url()).searchParams.get('workspace_root') === worktrees[1].path) {
        await route.fulfill({ json: [staleTab] });
      } else await route.fallback();
    };
    await page.route('**/api/terminals?*', staleList);
    const remote = { ...structuredClone(state.createdTerminalTabs[0]), id: 'new-cli', name: 'New worktree agent', workspace_root: worktrees[1].path };
    remote.panes[0].id = 'new-cli-pane';
    remote.panes[0].cwd = worktrees[1].path;
    remote.layout.pane_id = 'new-cli-pane';
    state.createdTerminalTabs.push(remote);
    await surface(page).getByRole('button', { name: 'CLI Terminals 显示范围', exact: true }).click();
    await page.getByRole('menuitemcheckbox').click();
    await surface(page).locator('.terminal-panel-open').filter({ hasText: remote.name }).click();
    await surface(page).getByRole('tab').filter({ hasText: remote.name }).waitFor();
    await surface(page).locator('.terminal-connection.observing').waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), worktrees[0].path);
    await page.unroute('**/api/terminals?*', staleList);
    assert.equal(state.createdTerminals.length, 0);
    assert.equal(await page.locator('.terminal-emulator-shell').count(), 3, 'only the explicitly opened CLI tab attaches');
    assert.equal(await page.getByRole('heading', { name: '无法打开 Tab' }).count(), 0);
    await page.reload();
    await surface(page).getByRole('tab').filter({ hasText: remote.name }).waitFor();
    await surface(page).locator('.terminal-connection.observing').waitFor();
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-active')), worktrees[0].path);
    await switchWorktree(page, 1);
    await surface(page).getByRole('tab').filter({ hasText: remote.name }).click();
    await surface(page).getByRole('button', { name: '移回当前工作区', exact: true }).click();
    await surface(page).locator('.terminal-connection.observing').waitFor();
  });

  await test('repository terminal scope ignores disabled in-flight loads and recovers from refresh errors', async t => {
    const { page, state } = await fixture(t);
    const list = surface(page).locator('.terminal-panel');
    const toggle = async () => {
      await list.getByRole('button', { name: 'User Terminals 显示范围', exact: true }).click();
      await page.getByRole('menuitemcheckbox').click();
    };
    let release;
    state.allTerminalsGate = new Promise(resolve => { release = resolve; });
    await toggle();
    await list.getByRole('status').first().waitFor();
    assert.equal(await list.getByRole('region', { name: 'CLI Terminals', exact: true }).getByRole('status').count(), 0);
    await eventually(() => state.terminalListScopes.includes(null));
    await toggle();
    release();
    state.allTerminalsGate = null;
    assert.equal(await list.locator('.terminal-worktree-heading').count(), 0);
    assert.equal(await list.locator('.terminal-panel-row').count(), 1);
    await toggle();
    await eventually(async () => await list.locator('.terminal-worktree-heading').count() === 2);
    state.agentFailure = true;
    await list.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await list.getByRole('alert').waitFor();
    assert.equal(await list.getByRole('region', { name: 'CLI Terminals', exact: true }).getByRole('alert').count(), 0);
    assert.equal(await list.locator('.terminal-panel-row').count(), 2, 'failed polls preserve the last successful inventory');
    state.agentFailure = false;
    await list.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await list.getByRole('alert').waitFor({ state: 'detached' });
    assert.equal(await list.locator('.terminal-panel-row').count(), 2);
  });

  await test('terminal panel reopens hidden instances, reuses tabs and destroys only after confirmation', async t => {
    const { page, state } = await fixture(t, { splitTerminal: true, floating: true });
    const list = surface(page).locator('.terminal-panel');
    const row = list.locator('.terminal-panel-open').filter({ hasText: 'Shell' });
    const floating = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const panes = surface(page).locator('.terminal-emulator-shell');
    await eventually(async () => await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').count() === 3);
    const closedBefore = state.closedTerminalConnections.length;
    const firstPane = await panes.first().elementHandle();
    await surface(page).getByRole('button', { name: '关闭 Shell', exact: true }).click();
    await eventually(async () => await panes.count() === 0);
    await eventually(() => Promise.resolve(new Set(state.closedTerminalConnections.slice(closedBefore).filter(url => url.includes('/terminals/wt-0-tab/'))).size === 3));
    assert.equal(await firstPane.evaluate(node => node.isConnected), false);
    assert.equal(await page.getByRole('alertdialog').count(), 0);
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(await row.getByRole('img', { name: '已隐藏', exact: true }).isVisible(), true);

    await list.getByRole('button', { name: '刷新 User Terminals', exact: true }).click();
    await page.reload();
    await row.waitFor();
    assert.equal(await surface(page).getByRole('tab').count(), 0, 'hidden state survives polling and reload');
    assert.equal(await panes.count(), 0);
    await row.click({ button: 'right' });
    const menu = page.getByRole('menu', { name: 'Shell 终端操作', exact: true });
    assert.deepEqual(await menu.getByRole('menuitem').allTextContents(), ['打开', '打开 · 浮动工作区', '销毁']);
    await page.keyboard.press('Escape');
    await menu.waitFor({ state: 'detached' });
    await list.getByRole('button', { name: 'Shell 终端操作', exact: true }).click();
    await menu.getByRole('menuitem', { name: '打开', exact: true }).click();
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').first().waitFor();
    assert.equal(await panes.count(), 3, 'all panes reconnect to the existing layout');
    const reopenedPane = await panes.first().elementHandle();
    await row.click();
    assert.equal(await surface(page).getByRole('tab').count(), 1);
    assert.equal(await reopenedPane.evaluate(node => node.isConnected), true, 'opening an existing tab reuses its view');
    assert.equal(state.createdTerminals.length, 0);

    await row.click({ button: 'right' });
    await menu.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await floating.locator('.terminal-emulator-shell:not(.restore-pending)').first().waitFor();
    assert.equal(await floating.getByRole('tab').count(), 1);
    assert.equal(await floating.locator('.terminal-emulator-shell').count(), 3);
    await floating.getByRole('button', { name: '最小化浮动工作区' }).click();
    await row.click();
    await floating.waitFor({ state: 'visible' });
    assert.equal(await floating.getByRole('tab').count(), 1, 'normal open reveals an existing floating tab');
    await floating.getByRole('button', { name: '最小化浮动工作区' }).click();

    await row.click({ button: 'right' });
    await menu.getByRole('menuitem', { name: '销毁', exact: true }).click();
    const confirmation = page.getByRole('alertdialog', { name: '销毁终端？', exact: true });
    await confirmation.getByRole('button', { name: '取消', exact: true }).click();
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(await row.isVisible(), true);
    await row.click({ button: 'right' });
    await menu.getByRole('menuitem', { name: '销毁', exact: true }).click();
    await confirmation.getByRole('button', { name: '销毁', exact: true }).click();
    await row.waitFor({ state: 'detached' });
    await eventually(async () => await floating.locator('.terminal-emulator-shell').count() === 0);
    assert.deepEqual(state.closedTerminalIds, ['wt-0-tab']);
    assert.equal(state.createdTerminals.length, 0);
    assert.equal(await floating.locator('[role=tab]').count(), 0);
  });

  await test('left sidebar can reopen with both sidebars hidden or without any projects', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '隐藏右侧栏', exact: true }).click();
    await page.getByRole('button', { name: '隐藏左侧栏', exact: true }).click();
    const center = await surface(page).locator('.project-aow-center').boundingBox();
    assert.equal(center.x, 0);
    assert.equal(center.width, page.viewportSize().width);
    await page.getByRole('button', { name: '显示右侧栏', exact: true }).click();
    await page.getByRole('button', { name: '显示左侧栏', exact: true }).click();
    state.projects = [];
    await page.getByRole('button', { name: '刷新全部', exact: true }).click();
    await page.getByRole('button', { name: '定位 Projects', exact: true }).click();
    await page.locator('.project-aow-onboarding').waitFor();
    await page.getByRole('button', { name: '隐藏左侧栏', exact: true }).click();
    await page.reload();
    await page.getByRole('button', { name: '显示左侧栏', exact: true }).click();
    await page.getByRole('button', { name: '定位 Projects', exact: true }).click();
    await page.locator('.project-aow-onboarding').getByRole('button', { name: '注册项目' }).click();
    await page.getByRole('dialog').waitFor();
  });

  await test('collapsible groups preserve active resources, keyboard interaction and worktree-specific state', async t => {
    const { page, state } = await fixture(t, { multipleTerminals: true });
    const terminalGroup = () => groupButton(surface(page), 'Terminal');
    const terminal = await surface(page).locator('.terminal-emulator-shell:visible').elementHandle();
    await terminalGroup().click();
    assert.equal(await terminalGroup().getAttribute('aria-expanded'), 'false');
    assert.equal(await terminalGroup().locator('.project-aow-group-count').textContent(), '2');
    assert.equal(await surface(page).getByRole('tab').count(), 0);
    assert.equal(await terminal.evaluate(element => element.isConnected && element.getBoundingClientRect().height > 0), true);
    state.agents['wt-0-pane'] = 'codex';
    state.titles['wt-0-pane'] = 'Still running';
    await surface(page).locator('.terminal-pane-name').filter({ hasText: 'Still running' }).waitFor();
    assert.equal(await terminalGroup().getAttribute('aria-expanded'), 'false', 'background terminal metadata does not unfold a group');
    await terminalGroup().focus();
    await page.keyboard.press('Enter');
    assert.equal(await surface(page).getByRole('tab').count(), 2);
    await terminalGroup().focus();
    await page.keyboard.press('Space');
    assert.equal(await terminalGroup().getAttribute('aria-expanded'), 'false');
    await switchWorktree(page, 1);
    await terminalGroup().waitFor();
    assert.equal(await terminalGroup().getAttribute('aria-expanded'), 'true');
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    await surface(page).locator('.tree-row[title="/workspace/wt-1/edit.txt"]').click();
    await surface(page).locator('.monaco-editor').first().waitFor();
    const fileGroup = () => groupButton(surface(page), '文件');
    const models = await modelPaths(page);
    await fileGroup().click();
    assert.equal(await fileGroup().locator('.project-aow-group-count').textContent(), '1');
    assert.deepEqual(await modelPaths(page), models);
    await page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      monaco.editor.getEditors().find(editor => editor.getDomNode()?.closest('.project-aow-surface:not([hidden])'))?.setValue('Edit while collapsed');
    });
    await eventually(() => Promise.resolve(state.textFiles['/workspace/wt-1/edit.txt'].content === 'Edit while collapsed'));
    assert.equal(await fileGroup().getAttribute('aria-expanded'), 'false', 'autosave does not unfold a group');
    await surface(page).locator('.tree-row[title="/workspace/wt-1/edit.txt"]').click();
    await eventually(async () => await fileGroup().getAttribute('aria-expanded') === 'true');
    await fileGroup().click();
    await terminalGroup().click();
    await page.reload();
    await fileGroup().waitFor();
    await surface(page).locator('.monaco-editor').first().waitFor();
    assert.equal(await fileGroup().getAttribute('aria-expanded'), 'false');
    assert.equal(await terminalGroup().getAttribute('aria-expanded'), 'false');
    assert.equal(await surface(page).getByRole('tab').count(), 0);
    await switchWorktree(page, 0);
    await terminalGroup().waitFor();
    assert.equal(await terminalGroup().getAttribute('aria-expanded'), 'false');
    assert.equal(await fileGroup().count(), 0, 'empty groups do not occupy the tab bar');
    assert.deepEqual(state.closedTerminalIds, []);
  });

  await test('floating collapsible groups restore independently and reveal explicit opens including existing tabs', async t => {
    const { page, state } = await fixture(t, { floating: true });
    await tab(page, 'Shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const terminalGroup = groupButton(panel, 'Terminal');
    const fileGroup = groupButton(panel, '文件');
    const tabs = panel.getByRole('tablist', { name: '浮动工作区标签' });
    await panel.locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    await terminalGroup.click();
    assert.equal(await tabs.getByRole('tab').count(), 0);
    await panel.locator('.terminal-emulator-shell:visible').waitFor();
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: '新建 Markdown', exact: true }).click();
    await tabs.getByRole('tab').filter({ hasText: '.tmp-fixture.md' }).waitFor();
    assert.equal(await terminalGroup.getAttribute('aria-expanded'), 'false');
    await fileGroup.click();
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    assert.equal(await groupButton(surface(page), 'Terminal').getAttribute('aria-expanded'), 'true');
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    await surface(page).locator('.tree-row[title="/workspace/wt-0/edit.txt"]').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await tabs.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    await fileGroup.click();
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await groupButton(surface(page), '文件').click();
    await surface(page).locator('.tree-row[title="/workspace/wt-0/edit.txt"]').click();
    await tabs.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    assert.equal(await fileGroup.getAttribute('aria-expanded'), 'true', 'opening an already active floating file reveals its collapsed group');
    assert.equal(await groupButton(surface(page), '文件').getAttribute('aria-expanded'), 'false', 'revealing a floating file preserves its source workspace group state');
    await fileGroup.click();
    if (process.env.GROUPS_SCREENSHOT) await page.screenshot({ path: process.env.GROUPS_SCREENSHOT });
    await page.reload();
    await fileGroup.waitFor();
    await panel.locator('.monaco-editor').first().waitFor();
    assert.equal(await terminalGroup.getAttribute('aria-expanded'), 'false');
    assert.equal(await fileGroup.getAttribute('aria-expanded'), 'false');
    assert.equal(await tabs.getByRole('tab').count(), 0);
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: 'Terminal', exact: true }).click();
    await tabs.getByRole('tab').filter({ hasText: 'New shell' }).waitFor();
    assert.equal(await terminalGroup.getAttribute('aria-expanded'), 'true');
    assert.equal(await fileGroup.getAttribute('aria-expanded'), 'false', 'a terminal placeholder must not unfold the file group');
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: '打开系统文件浏览器', exact: true }).click();
    const browserGroup = groupButton(panel, '系统文件浏览器');
    await browserGroup.click();
    await panel.getByRole('region', { name: '系统文件浏览器', exact: true }).waitFor();
    assert.equal(await browserGroup.locator('.project-aow-group-count').textContent(), '1');
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: '打开系统文件浏览器', exact: true }).click();
    await tabs.getByRole('tab').filter({ hasText: '系统文件浏览器' }).waitFor();
    assert.equal(await tabs.getByRole('tab').filter({ hasText: '系统文件浏览器' }).count(), 1);
    assert.deepEqual(state.closedTerminalIds, []);
  });

  await test('collapsed group badges cap at one character and group closes hide every terminal without destroying instances', async t => {
    const { page, state } = await fixture(t, { extraTerminals: 8 });
    const terminalGroup = groupButton(surface(page), 'Terminal');
    await terminalGroup.click();
    assert.equal(await terminalGroup.locator('.project-aow-group-count').textContent(), '9');
    await surface(page).getByRole('button', { name: '新建窗体', exact: true }).click();
    await surface(page).getByRole('button', { name: 'Terminal', exact: true }).click();
    await tab(page, 'New shell').waitFor();
    await terminalGroup.click();
    assert.equal(await terminalGroup.locator('.project-aow-group-count').textContent(), '…');
    assert.match(await terminalGroup.getAttribute('title'), /10 个 Tab/);
    if (process.env.GROUPS_OVERFLOW_SCREENSHOT) await page.screenshot({ path: process.env.GROUPS_OVERFLOW_SCREENSHOT });
    await terminalGroup.click();
    await tab(page, 'New shell').click({ button: 'right' });
    const menu = page.getByRole('menu', { name: 'New shell 操作', exact: true });
    await menu.waitFor();
    await surface(page).getByRole('tablist').evaluate(element => { element.scrollLeft = 0; });
    await menu.waitFor({ state: 'detached' });
    await tab(page, 'New shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '关闭所有', exact: true }).click();
    await terminalGroup.waitFor({ state: 'detached' });
    assert.equal(await page.getByRole('alertdialog').count(), 0);
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(await surface(page).getByRole('tab').count(), 0);
    assert.equal(await surface(page).locator('.terminal-emulator-shell').count(), 0);
    assert.equal(await surface(page).locator('.terminal-panel-row').count(), 10);
    await surface(page).getByRole('button', { name: '新建窗体', exact: true }).waitFor();
  });

  await test('floating pin preserves terminals, delays hiding, remembers preferences and respects minimization', async t => {
    const { page, state } = await fixture(t, { floating: true, clock: true });
    await tab(page, 'Shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const pin = panel.getByRole('button', { name: '固定显示浮动工作区', exact: true });
    await panel.locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    const terminal = await panel.locator('.terminal-emulator-shell').elementHandle();
    const bounds = await panel.boundingBox();
    const enter = () => page.mouse.move(bounds.x + 80, bounds.y + 90);
    const leave = () => page.mouse.move(5, 5);
    await page.clock.pauseAt('2026-09-15T01:00:00Z');
    assert.equal(await pin.getAttribute('aria-pressed'), 'true');
    await leave();
    await page.clock.runFor(400);
    assert.equal(await panel.isVisible(), true, 'pinned windows stay visible');
    await enter();
    await pin.dispatchEvent('click');
    await leave();
    await page.clock.runFor(100);
    assert.equal(await panel.isVisible(), true, 'brief excursions do not hide the window');
    await enter();
    await page.clock.runFor(150);
    assert.equal(await panel.isVisible(), true, 'returning cancels pending hiding');
    await leave();
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    assert.equal(await terminal.evaluate(element => element.isConnected), true);
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-floating-open')), 'true');
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-floating-pinned')), 'false');
    assert.equal(await page.evaluate(() => Boolean(document.activeElement?.closest('[data-floating-workspace]'))), false);
    assert.equal(await page.evaluate(({ x, y }) => Boolean(document.elementFromPoint(x + 80, y + 90)?.closest('[data-floating-workspace]')), bounds), false);
    await enter();
    await panel.waitFor();
    assert.deepEqual(await panel.boundingBox(), bounds);
    assert.equal(await terminal.evaluate(element => element.isConnected), true);
    await pin.dispatchEvent('click');
    await leave();
    await page.clock.runFor(400);
    assert.equal(await panel.isVisible(), true);
    await enter();
    await pin.dispatchEvent('click');
    await panel.getByRole('button', { name: '最小化浮动工作区', exact: true }).dispatchEvent('click');
    await leave();
    await enter();
    await page.clock.runFor(400);
    assert.equal(await panel.isVisible(), false, 'manual minimization disables hover reveal');
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-floating-open')), 'false');
    await page.getByRole('button', { name: '浮动工作区', exact: true }).dispatchEvent('click');
    await panel.waitFor();
    await page.clock.resume();
    await page.reload();
    await pin.waitFor();
    assert.equal(await pin.getAttribute('aria-pressed'), 'false');
    await panel.locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(state.createdTerminals.length, 0);
  });

  await test('floating pin keeps menus and dialogs usable and suspends hiding during dragging and resizing', async t => {
    const { page, state } = await fixture(t, { floating: true, clock: true });
    await tab(page, 'Shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await panel.locator('.terminal-emulator-shell:not(.restore-pending):visible').waitFor();
    const original = await panel.boundingBox();
    const enter = () => page.mouse.move(original.x + 80, original.y + 90);
    await enter();
    await page.clock.pauseAt('2026-09-15T01:00:00Z');
    await panel.getByRole('button', { name: '固定显示浮动工作区', exact: true }).dispatchEvent('click');
    await panel.getByRole('button', { name: '浮动工作区新建', exact: true }).focus();
    await panel.getByRole('button', { name: '浮动工作区新建', exact: true }).dispatchEvent('click');
    await page.mouse.move(5, 5);
    await page.clock.runFor(500);
    assert.equal(await panel.isVisible(), true, 'the new menu suspends hiding');
    await page.keyboard.press('Escape');
    await panel.locator('.project-aow-new-menu').waitFor({ state: 'detached' });
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await enter();
    await page.mouse.move(original.x + original.width - 5, original.y + 80);
    await panel.getByRole('tab').filter({ hasText: 'Shell' }).evaluate((element, bounds) => {
      element.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: bounds.x + bounds.width - 5, clientY: bounds.y + 80 }));
    }, original);
    const menu = page.getByRole('menu', { name: 'Shell 操作', exact: true });
    await menu.waitFor();
    const menuBounds = await menu.boundingBox();
    const menuX = menuBounds.x + menuBounds.width - 8;
    assert.ok(menuX > original.x + original.width, `the menu extends beyond the floating window: ${JSON.stringify({ original, menuBounds })}`);
    await page.mouse.move(menuX, menuBounds.y + 15);
    await page.clock.runFor(500);
    assert.equal(await panel.isVisible(), true, 'a portal menu is still part of the interaction');
    await page.mouse.click(5, 5);
    await menu.waitFor({ state: 'detached' });
    await surface(page).locator('.terminal-panel-open').filter({ hasText: 'Shell' }).dispatchEvent('contextmenu', { clientX: menuBounds.x, clientY: menuBounds.y });
    await page.getByRole('menuitem', { name: '销毁', exact: true }).dispatchEvent('click');
    const dialog = page.getByRole('alertdialog', { name: '销毁终端？', exact: true });
    await dialog.waitFor();
    await page.mouse.move(5, 5);
    await page.clock.runFor(500);
    assert.equal(await panel.isVisible(), true, 'modal confirmation suspends hiding');
    await dialog.getByRole('button', { name: '取消', exact: true }).dispatchEvent('click');
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await enter();
    await page.mouse.move(original.x + original.width - 190, original.y + 7);
    await page.mouse.down();
    await page.mouse.move(0, 0);
    await page.clock.runFor(500);
    assert.equal(await panel.isVisible(), true, 'dragging outside the bounds does not hide');
    const moved = await panel.boundingBox();
    assert.equal(moved.x, 8);
    assert.equal(moved.y, 8);
    await page.mouse.up();
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await page.mouse.move(original.x + original.width - 10, original.y + 50);
    await page.clock.runFor(220);
    assert.equal(await panel.isVisible(), false, 'the old bounds no longer reveal a moved window');
    await page.mouse.move(moved.x + 50, moved.y + 60);
    await panel.waitFor();
    const resize = await panel.getByRole('separator', { name: '调整浮动工作区大小', exact: true }).boundingBox();
    await page.mouse.move(resize.x + 5, resize.y + 5);
    await page.mouse.down();
    await page.mouse.move(0, 0);
    await page.clock.runFor(500);
    assert.equal(await panel.isVisible(), true, 'resizing outside the window does not hide');
    const resized = await panel.boundingBox();
    assert.equal(resized.width, 420);
    assert.equal(resized.height, 280);
    await page.mouse.up();
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await page.mouse.move(resized.x + 50, resized.y + 60);
    await panel.waitFor();
    assert.deepEqual(state.closedTerminalIds, []);
  });

  await test('floating pin preserves hidden edits, reveals explicit opens and uses maximized bounds', async t => {
    const { page, state } = await fixture(t, { floating: true, clock: true });
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await panel.getByRole('button', { name: '新建 Markdown', exact: true }).click();
    await panel.locator('.monaco-editor').first().waitFor();
    const bounds = await panel.boundingBox();
    const models = await modelPaths(page);
    await page.mouse.move(bounds.x + 80, bounds.y + 90);
    await page.clock.pauseAt('2026-09-15T01:00:00Z');
    await panel.getByRole('button', { name: '固定显示浮动工作区', exact: true }).dispatchEvent('click');
    await page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      monaco.editor.getEditors().find(editor => editor.getDomNode()?.closest('[data-floating-workspace]'))?.setValue('# Saved while hidden');
    });
    await page.mouse.move(5, 5);
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await page.clock.runFor(1200);
    await eventually(() => Promise.resolve(state.textFiles['/notes/localhost/test/__aow_floating/.tmp-fixture.md'].content === '# Saved while hidden'));
    assert.deepEqual(await modelPaths(page), models);
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).dispatchEvent('click');
    await page.clock.runFor(32);
    await surface(page).locator('.tree-row[title="/workspace/wt-0/edit.txt"]').evaluate(element => {
      element.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 1400, clientY: 100 }));
    });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).dispatchEvent('click');
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    await page.clock.runFor(400);
    assert.equal(await panel.isVisible(), true, 'explicit opening gives time to enter the window');
    await page.mouse.move(bounds.x + 80, bounds.y + 90);
    await panel.getByRole('button', { name: '最大化浮动工作区', exact: true }).dispatchEvent('click');
    await page.mouse.move(2, 2);
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await page.mouse.move(25, 100);
    await panel.waitFor();
    assert.equal((await panel.boundingBox()).width, 1424);
    await panel.getByRole('button', { name: '还原浮动工作区', exact: true }).dispatchEvent('click');
    await page.mouse.move(2, 2);
    await page.clock.runFor(220);
    await panel.waitFor({ state: 'hidden' });
    await page.mouse.move(25, 100);
    await page.clock.runFor(220);
    assert.equal(await panel.isVisible(), false, 'restoring replaces the maximized hover region');
    await page.mouse.move(bounds.x + 80, bounds.y + 90);
    await panel.waitFor();
    assert.deepEqual(await panel.boundingBox(), bounds);
  });

  await test('floating workspace uses the built-in project, persists tabs and keeps a single file browser', async t => {
    const { page, state } = await fixture(t, { floating: true });
    assert.equal(await page.locator('.project-aow-project-row').count(), 1);
    assert.equal(await page.locator('.project-aow-sidebar-footer').getByRole('button', { name: '系统文件浏览器' }).count(), 0);
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const originalBounds = await panel.boundingBox();
    const header = await panel.locator('.project-aow-center-tabs').boundingBox();
    await page.mouse.move(header.x + 60, header.y + 5);
    await page.mouse.down();
    await page.mouse.move(header.x + 85, header.y + 23);
    await page.mouse.up();
    const movedBounds = await panel.boundingBox();
    assert.equal(movedBounds.x, originalBounds.x + 25);
    assert.equal(movedBounds.y, originalBounds.y + 18);
    const resize = await panel.getByRole('separator', { name: '调整浮动工作区大小' }).boundingBox();
    await page.mouse.move(resize.x + 5, resize.y + 5);
    await page.mouse.down();
    await page.mouse.move(resize.x + 35, resize.y + 30);
    await page.mouse.up();
    const savedBounds = await panel.boundingBox();
    assert.equal(savedBounds.width, originalBounds.width + 30);
    assert.equal(savedBounds.height, originalBounds.height + 25);
    await panel.getByRole('button', { name: '最大化浮动工作区' }).click();
    assert.equal((await panel.boundingBox()).width, 1424);
    await panel.getByRole('button', { name: '还原浮动工作区' }).click();
    assert.deepEqual(await panel.boundingBox(), savedBounds);
    await panel.getByRole('button', { name: '新建 Markdown', exact: true }).click();
    await panel.getByRole('tab').filter({ hasText: '.tmp-fixture.md' }).waitFor();
    await panel.locator('.monaco-editor').first().waitFor();
    await page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      monaco.editor.getEditors().find(editor => editor.getDomNode()?.closest('[data-floating-workspace]'))?.setValue('# Global note');
    });
    await eventually(() => Promise.resolve(state.writes.some(write => write.path === '/notes/localhost/test/__aow_floating/.tmp-fixture.md' && write.content === '# Global note')));
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: 'Terminal', exact: true }).click();
    await eventually(() => Promise.resolve(state.createdTerminals.length === 1));
    assert.equal(state.createdTerminals[0].workspace_root, '/global');
    assert.equal(state.createdTerminals[0].cwd, '/global');
    for (let i = 0; i < 2; i++) {
      await panel.getByRole('button', { name: '浮动工作区新建' }).click();
      await panel.getByRole('button', { name: '打开系统文件浏览器', exact: true }).click();
    }
    assert.equal(await panel.getByRole('tab').filter({ hasText: '系统文件浏览器' }).count(), 1);
    await panel.getByRole('button', { name: '打开 edit.txt', exact: true }).click();
    await panel.locator('.monaco-editor').first().waitFor();
    await page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      monaco.editor.getEditors().find(editor => editor.getDomNode()?.closest('[data-floating-workspace]'))?.setValue('External edited');
    });
    await eventually(() => Promise.resolve(state.textFiles['/workspace/wt-0/edit.txt'].content === 'External edited'));
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await switchWorktree(page, 1);
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    await page.reload();
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    await panel.locator('.monaco-editor').first().waitFor();
    assert.ok(await page.evaluate(async () => { const { monaco } = await import('/src/features/editor/monaco.ts'); return monaco.editor.getModels().some(model => model.getValue() === 'External edited'); }));
    assert.deepEqual(await panel.boundingBox(), savedBounds);
    if (process.env.FLOATING_SCREENSHOT) await page.screenshot({ path: process.env.FLOATING_SCREENSHOT });
    await page.evaluate(() => localStorage.removeItem('aow-floating-tabs-v1'));
    await page.reload();
    await panel.getByRole('tab').filter({ hasText: 'New shell' }).waitFor();
    assert.equal(state.createdTerminals.length, 1, 'server terminals are recovered after browser tab storage is cleared');
  });

  await test('floating project entries retain repository context and shared file edits', async t => {
    const { page, state } = await fixture(t, { floating: true });
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    await surface(page).locator('.tree-row[title="/workspace/wt-0/edit.txt"]').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).waitFor();
    await panel.locator('.monaco-editor').first().waitFor();
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await switchWorktree(page, 1);
    await surface(page).getByRole('button', { name: 'Source Control', exact: true }).click();
    await surface(page).locator('.change-row').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.getByRole('tab').filter({ hasText: '(Diff)' }).waitFor();
    assert.equal(state.diffRequests.at(-1).repo, '/workspace/wt-1');
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).click();
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: '打开系统文件浏览器', exact: true }).click();
    await panel.getByRole('button', { name: '打开 edit.txt', exact: true }).click();
    await panel.locator('.monaco-editor').first().waitFor();
    const editorInput = panel.locator('.monaco-editor textarea').first();
    await editorInput.focus();
    await editorInput.press('ControlOrMeta+A');
    await editorInput.pressSequentially('Shared buffer');
    await eventually(() => Promise.resolve(state.textFiles['/workspace/wt-0/edit.txt'].content === 'Shared buffer'));
    assert.equal(state.writes.filter(write => write.content === 'Shared buffer').length, 1);
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).last().getByRole('button', { name: '关闭 edit.txt' }).click();
    await panel.getByRole('tab').filter({ hasText: 'edit.txt' }).click();
    await panel.locator('.monaco-editor').first().waitFor();
    assert.ok(await page.evaluate(async () => { const { monaco } = await import('/src/features/editor/monaco.ts'); return monaco.editor.getModels().some(model => model.getValue() === 'Shared buffer'); }));
  });

  await test('floating creation keeps project context and notes root migration refreshes open file versions', async t => {
    const { page, state } = await fixture(t, { floating: true });
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await surface(page).getByRole('button', { name: '新建窗体', exact: true }).click();
    await surface(page).getByRole('button', { name: '新建 Markdown', exact: true }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.getByRole('tab').filter({ hasText: '.tmp-fixture.md' }).waitFor();
    assert.ok(state.textFiles['/notes/.tmp-fixture.md']);
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: '新建 Markdown', exact: true }).click();
    await eventually(async () => await panel.getByRole('tab').filter({ hasText: '.tmp-fixture.md' }).count() === 2);
    const sourcePath = '/notes/localhost/test/__aow_floating/.tmp-fixture.md';
    const editor = panel.locator('.monaco-editor textarea').first();
    await editor.focus();
    await editor.pressSequentially('Before migration');
    await eventually(() => Promise.resolve(state.textFiles[sourcePath].content === 'Before migration'));
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const settings = page.locator('.project-aow-settings');
    await settings.getByRole('textbox', { name: 'Notes root', exact: true }).fill('/new-notes');
    await settings.getByRole('button', { name: '保存', exact: true }).click();
    await eventually(() => Promise.resolve(state.settings.notes_base === '/new-notes'));
    await settings.getByRole('button', { name: '关闭', exact: true }).click();
    assert.equal(await panel.isVisible(), false, 'migration does not reopen a minimized workspace');
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    await panel.locator('.editor-toolbar code').filter({ hasText: '/new-notes/localhost/test/__aow_floating/.tmp-fixture.md' }).waitFor();
    await editor.focus();
    await editor.press('ControlOrMeta+A');
    await editor.pressSequentially('After migration');
    await eventually(() => Promise.resolve(state.writes.some(write => write.path === '/new-notes/localhost/test/__aow_floating/.tmp-fixture.md' && write.content === 'After migration')));
    assert.equal(state.writes.at(-1).version, '"migrated-version"');
    assert.equal(await panel.getByRole('tab').filter({ hasText: '.tmp-fixture.md' }).count(), 2);
    assert.ok(state.projects.find(item => item.builtin).registered_path === '/global');
  });

  await test('an existing terminal moves to floating, retains rename, and closes without destroying its instance', async t => {
    const { page, state } = await fixture(t, { floating: true });
    await tab(page, 'Shell').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const terminalTab = panel.getByRole('tab').filter({ hasText: 'Shell' });
    await panel.locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    assert.equal(state.createdTerminals.length, 0);
    await terminalTab.dblclick();
    const name = panel.getByRole('textbox', { name: 'Terminal 名称', exact: true });
    await name.fill('Floating Shell');
    await name.press('Enter');
    await panel.getByRole('tab').filter({ hasText: 'Floating Shell' }).waitFor();
    await panel.getByRole('button', { name: '关闭 Floating Shell' }).click();
    await panel.getByRole('tab').waitFor({ state: 'detached' });
    assert.equal(await panel.locator('.terminal-emulator-shell').count(), 0);
    assert.equal(await page.getByRole('alertdialog').count(), 0);
    assert.deepEqual(state.closedTerminalIds, []);
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await surface(page).locator('.terminal-panel-open').filter({ hasText: 'Floating Shell' }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    assert.equal(await panel.getByRole('tab').filter({ hasText: 'Floating Shell' }).count(), 1);
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await groupButton(surface(page), 'Terminal').click();
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    await panel.getByRole('tab').filter({ hasText: 'Floating Shell' }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: '还原', exact: true }).click();
    assert.equal(await panel.getByRole('tab').count(), 0);
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    assert.equal(await groupButton(surface(page), 'Terminal').getAttribute('aria-expanded'), 'true');
    await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').waitFor();
    assert.equal(state.createdTerminals.length, 0);
  });

  await test('floating tab groups share previews and close only their own group across source worktrees', async t => {
    const { page, state } = await fixture(t, { floating: true, multipleTerminals: true, splitTerminal: true });
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    const tabs = panel.getByRole('tablist', { name: '浮动工作区标签' });
    for (const i of [0, 1]) {
      if (i) await switchWorktree(page, i);
      await tab(page, 'Shell').click({ button: 'right' });
      await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
      await panel.locator('.terminal-emulator-shell:not(.restore-pending):visible').first().waitFor();
      if (!i) await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    }
    const shells = tabs.getByRole('tab').filter({ hasText: 'Shell' });
    assert.equal(await shells.count(), 2);
    await shells.first().hover();
    const preview = page.getByRole('tooltip');
    await preview.waitFor();
    assert.deepEqual(await preview.locator('.terminal-tab-preview-pane span').allTextContents(), ['wt-0', 'wt-0', 'wt-0']);
    await page.keyboard.press('Escape');
    await panel.getByRole('button', { name: '浮动工作区新建' }).click();
    await panel.getByRole('button', { name: '新建 Markdown', exact: true }).click();
    await tabs.getByRole('tab').filter({ hasText: '.tmp-fixture.md' }).waitFor();
    await panel.locator('.monaco-editor').first().waitFor();
    assert.equal(await tabs.locator('.project-aow-group-toggle').count(), 2);
    assert.deepEqual(await tabs.locator('[role=tab] > span').allTextContents(), ['Shell', 'Shell', '.tmp-fixture.md']);
    await shells.first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '关闭其他', exact: true }).click();
    await eventually(async () => await shells.count() === 1);
    assert.equal(await page.getByRole('alertdialog').count(), 0);
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(await tabs.getByRole('tab').count(), 2, 'closing terminals preserves the file group');
    // Reopen the second source's existing instance, then hide both sources together.
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await surface(page).locator('.terminal-panel-open').filter({ hasText: 'Shell' }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await eventually(async () => await shells.count() === 2);
    await shells.first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: '关闭所有', exact: true }).click();
    await eventually(async () => await tabs.getByRole('tab').count() === 1);
    assert.equal(await page.getByRole('alertdialog').count(), 0);
    assert.deepEqual(state.closedTerminalIds, []);
    assert.equal(state.createdTerminals.length, 0);
    assert.equal(await tabs.locator('.project-aow-group-toggle').count(), 1);
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    for (const i of [0, 1]) {
      await switchWorktree(page, i);
      await tab(page, 'Background').waitFor();
    }
    assert.equal(state.closedTerminalIds.some(id => id.includes('background')), false, 'non-floating source tabs remain open');
  });

  await test('floating PR, session and automation tabs preserve their source after switching worktrees', async t => {
    const { page, state } = await fixture(t, { floating: true, registeredAgents: [{ id: 'codex', display_name: 'Codex', available: true, args: [], env: {} }] });
    const session = { id: 'codex:fixture', agent: 'codex', session_id: 'fixture', title: 'Floating session', cwd: worktrees[0].path, created_at: task.created_at, updated_at: task.updated_at };
    state.sessionFixtures = [session];
    const pr = { number: 42, title: 'Floating PR', status: 'open', source_branch: 'main', target_branch: 'develop', url: 'https://example.invalid/pr/42', author: { username: 'me', display_name: 'Me' }, description: 'PR fixture description', updated_at: task.updated_at, created_at: task.created_at, files: [], checks: [], reviewers: [], unresolved_threads: [], commits_count: 1, changes_count: 0 };
    const queries = [];
    await page.context().route('**/api/my-pull-requests**', async route => {
      const url = new URL(route.request().url());
      queries.push(Object.fromEntries(url.searchParams));
      await route.fulfill({ json: url.pathname.endsWith('/42') ? pr : { repository: worktrees[0].path, current_branch: 'main', current_user: pr.author, pull_requests: [pr] } });
    });
    await page.context().route('**/api/aow/agent-sessions/*/snapshot?**', async route => {
      const url = new URL(route.request().url());
      queries.push(Object.fromEntries(url.searchParams));
      await route.fulfill({ json: { ...session, captured_at: task.updated_at, status: 'completed', truncated: false, turns: [{ id: 'turn', status: 'completed', user: { text: 'Session fixture prompt' }, final: { text: 'Session fixture result' } }] } });
    });
    const panel = page.getByRole('dialog', { name: '浮动工作区', exact: true });
    await surface(page).getByRole('button', { name: 'Pull Requests', exact: true }).click();
    await surface(page).locator('.my-pr-row').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.getByRole('article', { name: 'PR #42 详情' }).waitFor();
    assert.equal(queries.at(-1).repo, worktrees[0].path);
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await surface(page).getByRole('button', { name: 'Conversation', exact: true }).click();
    await surface(page).locator('.project-aow-session-row').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.getByText('Session fixture result', { exact: true }).waitFor();
    assert.equal(queries.at(-1).worktree_path, worktrees[0].path);
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await surface(page).getByRole('button', { name: 'Automation', exact: true }).click();
    await surface(page).locator('.automation-panel-row').click({ button: 'right' });
    await page.getByRole('menuitem', { name: '打开 · 浮动工作区', exact: true }).click();
    await panel.locator('.automation-detail').waitFor();
    await panel.getByRole('button', { name: '最小化浮动工作区' }).click();
    await switchWorktree(page, 1);
    await page.getByRole('button', { name: '浮动工作区', exact: true }).click();
    await panel.getByRole('tab', { name: /PR #42/ }).click();
    await panel.getByRole('article', { name: 'PR #42 详情' }).waitFor();
    await panel.getByRole('tab', { name: /Floating session/ }).click();
    await panel.getByText('Session fixture result', { exact: true }).waitFor();
    assert.equal(await panel.getByRole('tablist', { name: '浮动工作区标签' }).getByRole('tab').count(), 3);
  });

  async function currentEditorWrap(page) {
    return page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      const editor = monaco.editor.getEditors().find(editor => editor.getDomNode()?.checkVisibility());
      return editor && {
        mode: editor.getOption(monaco.editor.EditorOption.wordWrap),
        column: editor.getOption(monaco.editor.EditorOption.wrappingInfo).wrappingColumn,
        content: editor.getValue(),
      };
    });
  }

  await test('editor word wrap follows global settings with independent temporary tab overrides', async t => {
    const text = 'long text '.repeat(100);
    const first = '/workspace/wt-0/edit.txt';
    const second = '/workspace/wt-0/other.json';
    const { page, state } = await fixture(t, { beforeOpen: async ({ state }) => {
      state.settings.editor = { word_wrap: true };
      state.textFiles[first] = { content: text, version: 'v1' };
      state.textFiles[second] = { content: text, version: 'v1' };
    } });
    const wrap = page.getByRole('button', { name: '自动换行', exact: true });
    const openFile = async path => {
      await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
      await surface(page).locator(`.tree-row[data-tree-path="${path}"]`).click();
      await wrap.waitFor();
    };
    const expectWrap = async enabled => {
      await eventually(async () => (await currentEditorWrap(page))?.mode === (enabled ? 'on' : 'off'));
      assert.equal(await wrap.getAttribute('aria-pressed'), String(enabled));
      assert.equal((await currentEditorWrap(page)).content, text);
      assert.equal((await currentEditorWrap(page)).column > 0, enabled, 'long lines wrap to the viewport without changing file content');
    };
    const saveGlobal = async enabled => {
      await page.getByRole('button', { name: 'Settings', exact: true }).click();
      const dialog = page.locator('.project-aow-settings');
      await dialog.getByRole('button', { name: /^Editor/ }).click();
      await dialog.getByRole('checkbox', { name: 'Word Wrap（自动换行）', exact: true }).setChecked(enabled);
      await dialog.getByRole('button', { name: '保存', exact: true }).click();
      await dialog.getByRole('status').waitFor();
      await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    };
    await openFile(first);
    await expectWrap(true);
    await wrap.click();
    await expectWrap(false);
    assert.deepEqual(state.settingsUpdates, [], 'tab toggles never write global settings');
    await openFile(second);
    await expectWrap(true);
    await saveGlobal(false);
    await expectWrap(false);
    await tab(page, 'edit.txt').click();
    await expectWrap(false);
    await saveGlobal(true);
    await expectWrap(false);
    assert.match(await wrap.getAttribute('title'), /当前 Tab/);
    await tab(page, 'other.json').click();
    await expectWrap(true);
    assert.match(await wrap.getAttribute('title'), /跟随全局设置/);
    await tab(page, 'edit.txt').click();
    await page.getByRole('button', { name: '关闭 edit.txt', exact: true }).click();
    await openFile(first);
    await expectWrap(true);
    await wrap.click();
    await expectWrap(false);
    await page.reload();
    await expectWrap(true);
    assert.deepEqual(state.settingsUpdates, [{ editor: { word_wrap: false } }, { editor: { word_wrap: true } }]);
    assert.equal(state.settings.notes_base, '/notes');
    assert.deepEqual(state.settings.execution_path, ['/usr/local/bin', '/usr/bin', '/bin']);
    assert.deepEqual(state.writes, [], 'display preferences must never save file contents');
  });

  await test('editor word wrap defaults off for legacy settings and failed saves keep the active default', async t => {
    const { page, state } = await fixture(t);
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    await surface(page).locator('.tree-row[data-tree-path="/workspace/wt-0/edit.txt"]').click();
    await eventually(async () => (await currentEditorWrap(page))?.mode === 'off');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /^Editor/ }).click();
    const checkbox = dialog.getByRole('checkbox', { name: 'Word Wrap（自动换行）', exact: true });
    assert.equal(await checkbox.isChecked(), false);
    await checkbox.check();
    state.failSettingsSave = true;
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: 'Settings write failed' }).waitFor();
    assert.equal(await checkbox.isChecked(), true, 'failed save retains the draft');
    assert.equal(await dialog.getByRole('status').count(), 0);
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    assert.equal((await currentEditorWrap(page)).mode, 'off');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await dialog.getByRole('button', { name: /^Editor/ }).click();
    assert.equal(await checkbox.isChecked(), false);
  });

  await test('review providers preview scripts read-only, upload as drafts, and preserve failed saves', async t => {
    const { page, state } = await fixture(t);
    const dialog = page.locator('.project-aow-settings');
    const open = async () => {
      await page.getByRole('button', { name: 'Settings', exact: true }).click();
      await dialog.getByRole('button', { name: /Pull Requests/ }).click();
      await dialog.locator('.review-provider-editor .monaco-editor').waitFor();
    };
    await open();
    assert.equal(await dialog.getByLabel('CLI 命令', { exact: true }).count(), 0);
    const preview = dialog.getByRole('textbox', { name: 'Provider Python 脚本预览', exact: true });
    const initial = state.reviewProviders.providers[0].script;
    await eventually(async () => (await currentEditorWrap(page))?.content === initial);
    assert.equal(await preview.isEditable(), false);
    await preview.focus();
    await page.keyboard.press('Control+A');
    await page.keyboard.press('Backspace');
    await page.keyboard.type('accidental typing');
    await page.keyboard.insertText('# accidental paste\n');
    assert.equal((await currentEditorWrap(page)).content, initial, 'typing, deletion and pasted input cannot change the script');
    assert.equal(await dialog.getByRole('button', { name: '保存', exact: true }).isDisabled(), true);
    await dialog.getByRole('button', { name: '添加 Provider', exact: true }).click();
    await dialog.getByLabel('Provider ID', { exact: true }).fill('custom');
    await dialog.getByLabel('Provider 显示名称', { exact: true }).fill('Custom Git');
    await dialog.getByLabel('Remote 域名', { exact: true }).fill('git.example.com');
    await eventually(async () => (await currentEditorWrap(page))?.content === '');
    const uploaded = '# uploaded adapter\nprint("hello 中文")\n';
    await dialog.getByLabel('上传 Provider 脚本', { exact: true }).setInputFiles({ name: 'adapter.py', mimeType: 'text/x-python', buffer: Buffer.from(uploaded) });
    await eventually(async () => (await currentEditorWrap(page))?.content === uploaded);
    assert.equal(await preview.isEditable(), false, 'uploaded scripts remain read-only');
    assert.equal(state.reviewWrites.length, 0, 'upload only edits the draft');
    await dialog.getByRole('button', { name: /Environment/ }).click();
    await dialog.getByRole('button', { name: /Pull Requests/ }).click();
    await eventually(async () => (await currentEditorWrap(page))?.content === uploaded);
    await dialog.getByRole('button', { name: '检查协议', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '协议检查通过' }).waitFor();
    assert.equal(state.reviewTests[0].script, uploaded);
    assert.equal(state.reviewWrites.length, 0, 'testing a draft does not save it');
    state.failReviewSave = true;
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '配置已更新' }).waitFor();
    assert.equal((await currentEditorWrap(page)).content, uploaded, 'failed saves keep the uploaded content');
    state.failReviewSave = false;
    await dialog.getByLabel('启用此 Provider', { exact: true }).uncheck();
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '已保存' }).waitFor();
    assert.equal(state.reviewProviders.providers[1].script, uploaded);
    assert.equal(state.reviewProviders.providers[1].enabled, false);
    assert.deepEqual(Object.keys(state.reviewProviders.providers[1]).sort(), ['enabled', 'hosts', 'id', 'name', 'script']);
    await page.screenshot({ path: '/tmp/aow-review-provider-settings.png' });
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await open();
    await dialog.getByLabel('选择 Provider', { exact: true }).selectOption('1');
    await eventually(async () => (await currentEditorWrap(page))?.content === uploaded);
    assert.equal(await dialog.getByLabel('启用此 Provider', { exact: true }).isChecked(), false);
    await dialog.getByLabel('上传 Provider 脚本', { exact: true }).setInputFiles({ name: 'replacement.py', mimeType: 'text/x-python', buffer: Buffer.from('# replacement\n') });
    await eventually(async () => (await currentEditorWrap(page))?.content === '# replacement\n');
    assert.equal(state.reviewProviders.providers[1].script, uploaded, 'upload does not replace the saved script');
    await dialog.getByRole('button', { name: '放弃修改', exact: true }).click();
    await dialog.getByLabel('选择 Provider', { exact: true }).selectOption('1');
    await eventually(async () => (await currentEditorWrap(page))?.content === uploaded);
    await dialog.getByRole('button', { name: '移除', exact: true }).click();
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').filter({ hasText: '已保存' }).waitFor();
    assert.equal(state.reviewProviders.providers.length, 1);
    assert.equal(state.settingsUpdates.length, 0, 'provider settings preserve other settings');
  });

  await test('review remote selection keeps equal PR numbers in distinct provider tabs', async t => {
    const { page, state } = await fixture(t);
    state.reviewTargets = [
      { remote: 'origin', host: 'github.com', repository: 'team/project', provider: 'github', provider_name: 'GitHub' },
      { remote: 'upstream', host: 'git.example.com', repository: 'team/project', provider: 'custom', provider_name: 'Custom Git' },
    ];
    const queries = [];
    await page.context().route('**/api/my-pull-requests**', async route => {
      const url = new URL(route.request().url());
      const target = state.reviewTargets.find(t => t.remote === url.searchParams.get('remote'));
      queries.push(Object.fromEntries(url.searchParams));
      assert.ok(target, 'an explicit remote must be selected before loading');
      assert.equal(url.searchParams.get('provider'), target.provider);
      const pr = { ...target, number: 42, title: target.provider_name + ' review', status: 'open', source_branch: 'main', target_branch: 'develop', url: null, author: { username: 'me', display_name: 'Me' }, description: 'Fixture', files: [], checks: [], reviewers: [], unresolved_threads: [], commits_count: 1, changes_count: 0, created_at: '', updated_at: '' };
      await route.fulfill({ json: url.pathname.endsWith('/42') ? pr : { repository: worktrees[0].path, current_branch: 'main', current_user: pr.author, pull_requests: [pr] } });
    });
    await surface(page).getByRole('button', { name: 'Pull Requests', exact: true }).click();
    const picker = surface(page).getByLabel('PR remote', { exact: true });
    await picker.waitFor();
    assert.equal(queries.length, 0);
    for (const remote of ['origin', 'upstream']) {
      await picker.selectOption(remote);
      await surface(page).locator('.my-pr-row').filter({ hasText: remote === 'origin' ? 'GitHub review' : 'Custom Git review' }).waitFor();
      assert.equal(await surface(page).locator('.my-pr-row.selected').count(), 0);
      await surface(page).locator('.my-pr-row').click();
      await surface(page).getByRole('heading', { name: remote === 'origin' ? 'GitHub review' : 'Custom Git review', exact: true }).waitFor();
      assert.equal(new URL(page.url()).searchParams.get('remote'), remote);
    }
    const tabs = surface(page).getByRole('tab', { name: /PR #42/ });
    assert.equal(await tabs.count(), 2);
    await tabs.first().click();
    await page.waitForURL(url => url.pathname.includes('/review/github/42') && url.searchParams.get('remote') === 'origin');
    await page.reload();
    await surface(page).getByRole('heading', { name: 'GitHub review', exact: true }).waitFor();
    assert.equal(new URL(page.url()).searchParams.get('remote'), 'origin');
  });

  await test('global execution PATH can be saved, reopened, and rediscovered without overwriting Notes', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /Environment/ }).click();
    const input = dialog.getByRole('textbox', { name: 'PATH 目录', exact: true });
    await eventually(async () => await input.inputValue() === '/usr/local/bin\n/usr/bin\n/bin');
    await input.fill('/opt/python/bin\n/usr/bin\n/bin');
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.settingsUpdates, [{ execution_path: ['/opt/python/bin', '/usr/bin', '/bin'] }]);
    assert.equal(state.settings.notes_base, '/notes');
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await dialog.getByRole('button', { name: /Environment/ }).click();
    await eventually(async () => await input.inputValue() === '/opt/python/bin\n/usr/bin\n/bin');
    await dialog.getByRole('button', { name: '从本机环境读取', exact: true }).click();
    await eventually(async () => await input.inputValue() === '/discovered/bin\n/usr/bin\n/bin');
    assert.equal(state.settingsUpdates.length, 1, 'discovery only edits the draft until Save');
    await input.fill('python3.11');
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByText('PATH 每行填写一个服务端绝对目录路径，不使用冒号分隔。', { exact: true }).waitFor();
    assert.equal(state.settingsUpdates.length, 1);
  });

  await test('detected agents can be edited, saved with their original identity, reopened, and reset', async t => {
    const agent = { id: 'codex', display_name: 'Codex', source: 'detected', available: true, command: 'codex', executable: '/usr/bin/codex', args: [], env: {} };
    const { page, state } = await fixture(t, { registeredAgents: [agent] });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /Agents/ }).click();
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    assert.equal(await dialog.getByLabel('Display name', { exact: true }).inputValue(), 'Codex');
    assert.equal(await dialog.getByLabel('Executable', { exact: true }).inputValue(), 'codex');
    await dialog.getByLabel('Arguments', { exact: true }).fill('--model\nmodel with spaces');
    assert.equal(await dialog.getByLabel('Environment keys', { exact: true }).count(), 0);
    const env = { BASE_URL: 'https://example.com/api?a=b', EMPTY: '', HOME: '/agent/home' };
    await dialog.getByLabel('Environment variables', { exact: true }).fill(JSON.stringify(env));
    await dialog.getByRole('button', { name: '保存配置', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.agentUpdates, [{ id: 'codex', agent_type: 'codex', display_name: 'Codex', command: 'codex', args: ['--model', 'model with spaces'], env }]);
    assert.equal(await dialog.locator('.project-aow-agent-row').count(), 1);
    await dialog.getByText('Configured', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await dialog.getByRole('button', { name: /Agents/ }).click();
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    assert.equal(await dialog.getByLabel('Executable', { exact: true }).inputValue(), 'codex');
    assert.equal(await dialog.getByLabel('Arguments', { exact: true }).inputValue(), '--model\nmodel with spaces');
    assert.deepEqual(JSON.parse(await dialog.getByLabel('Environment variables', { exact: true }).inputValue()), env);
    await dialog.getByRole('button', { name: '移除配置', exact: true }).click();
    await dialog.getByText('Auto detected', { exact: true }).waitFor();
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    assert.equal(await dialog.getByLabel('Arguments', { exact: true }).inputValue(), '');
    assert.equal(await dialog.getByLabel('Environment variables', { exact: true }).inputValue(), '{}');
    await dialog.getByRole('button', { name: '取消编辑', exact: true }).click();
    assert.equal(await dialog.getByRole('button', { name: '注册', exact: true }).isDisabled(), true);
    await dialog.getByLabel('Agent 类型', { exact: true }).selectOption('codex');
    await dialog.getByLabel('Display name', { exact: true }).fill('New agent');
    await dialog.getByLabel('Executable', { exact: true }).fill('new-agent');
    await dialog.getByLabel('Environment variables', { exact: true }).fill('   ');
    await dialog.getByRole('button', { name: '注册', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.equal(Object.hasOwn(state.agentUpdates.at(-1), 'id'), false, 'registering after editing must create a new ID');
    assert.deepEqual(state.agentUpdates.at(-1).env, {}, 'blank environment only inherits the launch environment');
    assert.equal(state.registeredAgents.length, 2);
  });

  await test('agent arguments normalize pasted commands and preserve literal values through save and reopen', async t => {
    const agent = { id: 'codex', display_name: 'Codex', source: 'detected', available: true, command: 'codex', executable: '/usr/bin/codex', args: [], env: {} };
    const { page, state } = await fixture(t, { registeredAgents: [agent] });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /Agents/ }).click();
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    const input = dialog.getByRole('textbox', { name: 'Arguments', exact: true });
    assert.equal(await input.evaluate(element => element.tagName), 'TEXTAREA');
    const command = ['codex -m gpt-6-luna \\', `  -c 'approval_policy="on-request"' \\`, `  -c 'approvals_reviewer="auto_review"'`].join('\r\n');
    await input.focus();
    await input.evaluate((element, text) => {
      const clipboardData = new DataTransfer();
      clipboardData.setData('text/plain', text);
      element.dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }));
    }, command);
    const expected = ['-m', 'gpt-6-luna', '-c', 'approval_policy="on-request"', '-c', 'approvals_reviewer="auto_review"'];
    assert.equal(await input.inputValue(), expected.join('\n'));
    await dialog.getByRole('button', { name: '保存配置', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.agentUpdates.at(-1).args, expected);
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    assert.equal(await input.inputValue(), expected.join('\n'));
    await input.fill(`${expected.join('\n')}\n--message\ntext with spaces and "quotes"`);
    await dialog.getByRole('button', { name: '保存配置', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.agentUpdates.at(-1).args, [...expected, '--message', 'text with spaces and "quotes"']);
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    await input.scrollIntoViewIfNeeded();
    await page.screenshot({ path: '/tmp/aow-agent-arguments-desktop.png' });
    await page.setViewportSize({ width: 390, height: 844 });
    await input.scrollIntoViewIfNeeded();
    assert.equal(await dialog.evaluate(element => element.scrollWidth <= element.clientWidth), true);
    assert.equal(await input.evaluate(element => element.getBoundingClientRect().right <= window.innerWidth), true);
    await page.screenshot({ path: '/tmp/aow-agent-arguments-mobile.png' });
  });

  await test('agent arguments keep typing uninterrupted and normalize on blur or keyboard submit', async t => {
    const agent = { id: 'codex', display_name: 'Codex', source: 'detected', available: true, command: 'codex', executable: '/usr/bin/codex', args: [], env: {} };
    const { page, state } = await fixture(t, { registeredAgents: [agent] });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /Agents/ }).click();
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    const input = dialog.getByRole('textbox', { name: 'Arguments', exact: true });
    await input.focus();
    await input.pressSequentially('-m gpt-6-luna');
    assert.equal(await input.inputValue(), '-m gpt-6-luna');
    await input.press('Tab');
    assert.equal(await input.inputValue(), '-m\ngpt-6-luna');
    await input.fill('--message "hello world"');
    await input.evaluate(element => element.form.requestSubmit());
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.agentUpdates.at(-1).args, ['--message', 'hello world']);
    await dialog.getByRole('button', { name: '编辑 Codex', exact: true }).click();
    const start = '--message\n'.length;
    await input.evaluate((element, start) => {
      element.setSelectionRange(start, start + 'hello'.length);
      const clipboardData = new DataTransfer();
      clipboardData.setData('text/plain', 'a new');
      element.dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }));
    }, start);
    assert.equal(await input.inputValue(), '--message\na new world');
    await dialog.getByRole('button', { name: '保存配置', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.agentUpdates.at(-1).args, ['--message', 'a new world']);
  });

  await test('agent registration requires a supported type and shows type icons for custom configurations', async t => {
    const registeredAgents = ['codex', 'claude', 'traecli', 'codex'].map((agent_type, index) => ({
      id: `custom-${index}`, agent_type, display_name: `Profile ${index}`, source: 'configured',
      available: true, command: '/opt/wrapper', executable: '/opt/wrapper', args: [], env: {},
    }));
    const { page, state } = await fixture(t, { registeredAgents });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /Agents/ }).click();
    const rows = dialog.locator('.project-aow-agent-row');
    assert.equal(await rows.count(), 4, 'multiple configurations of the same type are listed');
    const icons = await rows.locator('img.agent-icon').evaluateAll(images => images.map(image => image.getAttribute('src')));
    assert.equal(icons.length, 4);
    assert.ok(icons[0].includes('codex.png'));
    assert.ok(icons[1].includes('claude.png'));
    assert.notEqual(icons[2], icons[0]);
    assert.notEqual(icons[2], icons[1]);
    assert.equal(icons[3], icons[0]);
    const type = dialog.getByRole('combobox', { name: 'Agent 类型', exact: true });
    assert.deepEqual(await type.locator('option').evaluateAll(options => options.map(option => option.value)), ['', 'claude', 'codex', 'traecli']);
    await dialog.getByLabel('Display name', { exact: true }).fill('My custom wrapper');
    await dialog.getByLabel('Executable', { exact: true }).fill('/opt/custom/start');
    await dialog.getByLabel('Arguments', { exact: true }).fill('--anything\nvalue with spaces');
    const env = { CUSTOM_URL: 'https://example.com', EMPTY: '' };
    await dialog.getByLabel('Environment variables', { exact: true }).fill(JSON.stringify(env));
    assert.equal(await dialog.getByRole('button', { name: '注册', exact: true }).isDisabled(), true);
    assert.equal(await type.evaluate(element => element.validity.valueMissing), true);
    assert.equal(state.agentUpdates.length, 0);
    await type.selectOption('codex');
    await type.selectOption('traecli');
    await dialog.getByRole('button', { name: '注册', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.deepEqual(state.agentUpdates, [{ agent_type: 'traecli', display_name: 'My custom wrapper', command: '/opt/custom/start', args: ['--anything', 'value with spaces'], env }]);
    const row = rows.filter({ hasText: 'My custom wrapper' });
    assert.equal(await row.locator('img.agent-icon').getAttribute('src'), icons[2]);
    await dialog.getByRole('button', { name: '编辑 My custom wrapper', exact: true }).click();
    assert.equal(await type.inputValue(), 'traecli');
    await type.selectOption('claude');
    await dialog.getByRole('button', { name: '保存配置', exact: true }).click();
    await dialog.getByRole('status').waitFor();
    assert.equal(await row.locator('img.agent-icon').getAttribute('src'), icons[1]);
    assert.equal(state.registeredAgents.length, 5);
    await page.screenshot({ path: '/tmp/aow-agent-types.png' });
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await surface(page).getByRole('button', { name: '新建窗体', exact: true }).click();
    const menu = surface(page).locator('.project-aow-new-menu');
    assert.equal(await menu.getByRole('button', { name: 'My custom wrapper', exact: true }).locator('img.agent-icon').getAttribute('src'), icons[1]);
  });

  await test('unavailable configured agents retain editable commands and invalid or failed saves preserve the draft', async t => {
    const agent = { id: 'custom-missing', display_name: 'Missing agent', source: 'configured', available: false, command: 'missing-cli', executable: null, args: ['--existing'], env: { BASE_URL: 'https://example.com' } };
    const { page, state } = await fixture(t, { registeredAgents: [agent] });
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    const dialog = page.locator('.project-aow-settings');
    await dialog.getByRole('button', { name: /Agents/ }).click();
    await dialog.getByRole('button', { name: '编辑 Missing agent', exact: true }).click();
    assert.equal(await dialog.getByLabel('Agent 类型', { exact: true }).inputValue(), '');
    assert.equal(await dialog.getByRole('button', { name: '保存配置', exact: true }).isDisabled(), true);
    await dialog.getByLabel('Agent 类型', { exact: true }).selectOption('traecli');
    assert.equal(await dialog.getByLabel('Executable', { exact: true }).inputValue(), 'missing-cli');
    const args = dialog.getByLabel('Arguments', { exact: true });
    const env = dialog.getByLabel('Environment variables', { exact: true });
    const save = dialog.getByRole('button', { name: '保存配置', exact: true });
    await args.fill('--model "incomplete');
    await save.click();
    await dialog.getByText('Arguments 中的引号未闭合，请补全后再保存。', { exact: true }).waitFor();
    assert.equal(await args.inputValue(), '--model "incomplete');
    assert.equal(state.agentUpdates.length, 0);
    await args.fill('--changed');
    for (const invalid of ['[]', 'null', '{"KEY":42}', '{"BAD=KEY":"value"}']) {
      await env.fill(invalid);
      await save.click();
      await dialog.getByText('Environment variables 必须是变量名到字符串值的 JSON 对象', { exact: true }).waitFor();
      assert.equal(state.agentUpdates.length, 0);
    }
    await env.fill('{"KEY":"value"}');
    state.failAgentSave = true;
    await save.click();
    await dialog.getByText('Agent configuration write failed', { exact: true }).waitFor();
    assert.equal(await args.inputValue(), '--changed');
    assert.equal(await env.inputValue(), '{"KEY":"value"}');
    assert.deepEqual(state.registeredAgents, [agent]);
    state.failAgentSave = false;
    await save.click();
    await dialog.getByRole('status').waitFor();
    assert.equal(state.registeredAgents[0].id, agent.id);
    assert.deepEqual(state.registeredAgents[0].args, ['--changed']);
  });

  await test('terminal tabs detect all agents, including unselected tabs, and restore shell icons on exit', async t => {
    const { page, state } = await fixture(t, { multipleTerminals: true });
    assert.equal(await tab(page, 'Shell').locator('img.agent-icon').count(), 0);
    assert.equal(await tab(page, 'Background').getAttribute('aria-selected'), 'false');
    for (const [agent, asset] of [['codex', 'codex.png'], ['claude', 'claude.png'], ['traecli', 'trae.png']]) {
      state.agents = { 'wt-0-pane': agent, 'wt-0-background-pane': agent };
      await eventually(async () => await tab(page, 'Background').locator('img.agent-icon').count() === 1
        && (await tab(page, 'Background').locator('img.agent-icon').getAttribute('src')).includes(asset));
      assert.ok((await tab(page, 'Shell').locator('img.agent-icon').getAttribute('src')).includes(asset));
      assert.equal(await tab(page, 'Shell').getAttribute('aria-selected'), 'true');
    }
    for (const unknown of ['traex', 'future-agent']) {
      state.agents = { 'wt-0-pane': null, 'wt-0-background-pane': unknown };
      await eventually(async () => await tab(page, 'Background').locator('svg.lucide-bot').count() === 1);
      assert.equal(await tab(page, 'Background').locator('img.agent-icon').count(), 0);
    }
    state.agents = { 'wt-0-pane': null, 'wt-0-background-pane': null };
    await eventually(async () => await tab(page, 'Background').locator('svg.lucide-square-terminal').count() === 1);
    assert.equal(await tab(page, 'Shell').locator('svg.lucide-square-terminal').count(), 1);
    assert.equal(await tab(page, 'Background').locator('svg.lucide-square-terminal').count(), 1);
  });

  await test('terminal agent detection recovers after unavailable metadata without disrupting terminal tabs', async t => {
    const { page, state } = await fixture(t);
    state.agents = { 'wt-0-pane': 'codex' };
    await eventually(async () => await tab(page, 'Shell').locator('img.agent-icon').count() === 1);
    state.agentFailure = true;
    await eventually(() => state.failedAgentRequests > 0);
    // A failed metadata poll preserves the last known agent until a successful response.
    assert.ok((await tab(page, 'Shell').locator('img.agent-icon').getAttribute('src')).endsWith('codex.png'));
    state.agentFailure = false;
    state.agents = { 'wt-0-pane': 'claude' };
    await eventually(async () => (await tab(page, 'Shell').locator('img.agent-icon').getAttribute('src'))?.endsWith('claude.png'));
    assert.ok((await tab(page, 'Shell').locator('img.agent-icon').getAttribute('src')).endsWith('claude.png'));
    assert.equal(await tab(page, 'Shell').getAttribute('aria-selected'), 'true');
    assert.equal(await surface(page).locator('.terminal-emulator-shell:not(.restore-pending)').count(), 1);
  });

  await test('leaving unchanged tab names never disables live OSC titles', async t => {
    const { page, state } = await fixture(t);
    state.agents = { 'wt-0-pane': 'codex' };
    state.titles = { 'wt-0-pane': '任务 0' };
    for (const [index, exit] of ['blur', 'Enter', 'Escape'].entries()) {
      await eventually(async () => await tab(page, `任务 ${index}`).count() === 1);
      await tab(page, `任务 ${index}`).dblclick();
      const input = page.getByRole('textbox', { name: 'Terminal 名称', exact: true });
      assert.equal(await input.inputValue(), `任务 ${index}`);
      // OSC can update during editing; compare with the name at edit entry.
      state.titles['wt-0-pane'] = `任务 ${index + 1}`;
      await delay(1700);
      if (exit === 'blur') await input.evaluate(element => element.blur());
      else await input.press(exit);
      await eventually(async () => await tab(page, `任务 ${index + 1}`).count() === 1);
      assert.deepEqual(state.names, {});
    }
    await page.reload();
    await eventually(async () => await tab(page, '任务 3').count() === 1);
    assert.deepEqual(state.names, {});
  });

  await test('agent titles update background tabs, survive reload, and respect manual names and agent exit', async t => {
    const { page, state } = await fixture(t, { multipleTerminals: true });
    const terminalCount = await surface(page).locator('.terminal-emulator-shell').count();
    state.agents = { 'wt-0-pane': 'codex', 'wt-0-background-pane': 'traecli' };
    state.titles = { 'wt-0-pane': '实现登录', 'wt-0-background-pane': '后台重构' };
    await eventually(async () => await tab(page, '后台重构').count() === 1);
    assert.equal(await tab(page, '实现登录').getAttribute('aria-selected'), 'true');
    assert.equal(await tab(page, '后台重构').getAttribute('aria-selected'), 'false');
    assert.equal(await surface(page).locator('.terminal-emulator-shell').count(), terminalCount);
    await tab(page, '实现登录').dblclick();
    const rename = page.getByRole('textbox', { name: 'Terminal 名称', exact: true });
    await rename.fill('我的终端');
    await rename.press('Enter');
    await eventually(async () => state.names['wt-0-tab'] === '我的终端');
    state.titles = { 'wt-0-pane': '自动更新', 'wt-0-background-pane': '新的任务' };
    await eventually(async () => await tab(page, '新的任务').count() === 1);
    assert.equal(await tab(page, '我的终端').count(), 1);
    await page.reload();
    await eventually(async () => await tab(page, '新的任务').count() === 1);
    assert.equal(await tab(page, '我的终端').count(), 1);
    delete state.titles['wt-0-background-pane']; // Empty OSC title restores the default.
    await eventually(async () => await tab(page, 'Background').count() === 1);
    state.agents['wt-0-background-pane'] = null;
    state.titles['wt-0-background-pane'] = 'shell prompt title';
    await delay(1700);
    assert.equal(await tab(page, 'Background').count(), 1);
    assert.equal(await tab(page, 'shell prompt title').count(), 0);
    await switchWorktree(page, 1);
    await eventually(async () => await tab(page, 'Shell').count() === 1);
    assert.equal(await tab(page, '新的任务').count(), 0);
  });

  await test('split terminal tabs summarize agents while each pane keeps its own icon and title', async t => {
    const { page, state } = await fixture(t, { splitTerminal: true });
    const panes = surface(page).locator('.terminal-pane');
    const names = panes.locator('.terminal-pane-name');
    const terminalCount = await surface(page).locator('.terminal-emulator-shell').count();
    state.agents = { 'wt-0-pane': 'codex', 'wt-0-review': 'claude', 'wt-0-logs': null };
    state.titles = { 'wt-0-pane': '修复登录逻辑', 'wt-0-review': '审查鉴权实现', 'wt-0-logs': 'shell prompt' };
    await eventually(async () => await tab(page, '2 agents').count() === 1);
    assert.equal(await tab(page, '2 agents').locator(':scope > span').textContent(), '2 agents');
    assert.equal(await tab(page, '2 agents').locator('svg.lucide-columns2').count(), 1);
    assert.deepEqual(await names.allTextContents(), ['修复登录逻辑', '审查鉴权实现', 'wt-0']);
    assert.ok((await panes.nth(0).locator('.terminal-pane-header img.agent-icon').getAttribute('src')).endsWith('codex.png'));
    assert.ok((await panes.nth(1).locator('.terminal-pane-header img.agent-icon').getAttribute('src')).endsWith('claude.png'));
    assert.equal(await panes.nth(2).locator('.terminal-pane-header svg.lucide-square-terminal').count(), 1);
    await tab(page, '2 agents').hover();
    const preview = page.getByRole('tooltip');
    await preview.waitFor();
    assert.deepEqual(await preview.locator('.terminal-tab-preview-pane span').allTextContents(), ['修复登录逻辑', '审查鉴权实现', 'wt-0']);
    assert.equal(await preview.locator('img.agent-icon').count(), 2);
    if (process.env.AOW_TEST_SCREENSHOT) await page.screenshot({ path: process.env.AOW_TEST_SCREENSHOT });
    await page.keyboard.press('Escape');
    await preview.waitFor({ state: 'hidden' });
    for (let i = 0; i < 3; i++) {
      await names.nth(i).click();
      assert.equal(await tab(page, '2 agents').count(), 1, 'pane focus does not rename its group');
    }
    state.agents['wt-0-review'] = 'codex';
    await eventually(async () => await tab(page, '2 agents').locator('img.agent-icon').count() === 1);
    assert.ok((await tab(page, '2 agents').locator('img.agent-icon').getAttribute('src')).endsWith('codex.png'));
    state.agents['wt-0-pane'] = null;
    await eventually(async () => await tab(page, '审查鉴权实现').count() === 1);
    assert.equal(await names.nth(0).textContent(), 'wt-0');
    state.agents['wt-0-review'] = null;
    await eventually(async () => await tab(page, 'Shell').count() === 1);
    assert.equal(await tab(page, 'Shell').locator('svg.lucide-square-terminal').count(), 1);
    assert.equal(await panes.locator('.terminal-pane-header img.agent-icon').count(), 0);
    assert.deepEqual(await names.allTextContents(), ['wt-0', 'wt-0', 'wt-0']);
    assert.equal(await surface(page).locator('.terminal-emulator-shell').count(), terminalCount);
  });

  await test('manual tab names survive while legacy pane names are ignored, including maximized panes', async t => {
    const { page, state } = await fixture(t, { splitTerminal: true });
    state.paneNames = { 'wt-0-pane': 'Old saved pane title', 'wt-0-review': 'Old review name' };
    await page.reload();
    const panes = surface(page).locator('.terminal-pane');
    state.agents = { 'wt-0-pane': 'codex', 'wt-0-review': 'claude' };
    state.titles = { 'wt-0-pane': '实现功能', 'wt-0-review': '代码审查' };
    await eventually(async () => await tab(page, '2 agents').count() === 1);
    await tab(page, '2 agents').dblclick();
    const renameTab = page.getByRole('textbox', { name: 'Terminal 名称', exact: true });
    await renameTab.fill('我的分组');
    await renameTab.press('Enter');
    await eventually(async () => state.names['wt-0-tab'] === '我的分组');
    state.titles['wt-0-pane'] = '继续实现';
    await eventually(async () => await panes.nth(0).locator('.terminal-pane-name').textContent() === '继续实现');
    assert.equal(await tab(page, '我的分组').locator('.terminal-tab-agent-count').textContent(), '2');
    assert.equal(await page.getByRole('button', { name: '重命名 shell 窗口', exact: true }).count(), 0);
    await panes.nth(0).locator('.terminal-pane-name').dblclick();
    assert.equal(await page.getByRole('textbox', { name: 'shell 窗口名称', exact: true }).count(), 0);
    state.titles = { 'wt-0-pane': '新的任务', 'wt-0-review': '继续审查' };
    await eventually(async () => await panes.nth(0).locator('.terminal-pane-name').textContent() === '新的任务');
    await eventually(async () => await panes.nth(1).locator('.terminal-pane-name').textContent() === '继续审查');
    await panes.nth(1).getByRole('button', { name: '最大化 shell 窗口', exact: true }).click();
    const maximizedTabs = surface(page).locator('.terminal-pane-tabs');
    assert.deepEqual(await maximizedTabs.locator('button > span:last-child').allTextContents(), ['新的任务', '继续审查', 'wt-0']);
    assert.equal(await maximizedTabs.locator('img.agent-icon').count(), 2);
    assert.equal(await tab(page, '我的分组').count(), 1);
    await page.reload();
    await eventually(async () => await panes.nth(1).locator('.terminal-pane-name').textContent() === '继续审查');
    assert.equal(await panes.nth(0).locator('.terminal-pane-name').textContent(), '新的任务');
    assert.equal(await tab(page, '我的分组').locator('.terminal-tab-agent-count').textContent(), '2');
  });

  await test('session Resume menus launch each matching profile in a new active tab', async t => {
    const registeredAgents = ['codex', 'codex', 'claude', 'traecli', 'codex'].map((agent_type, index) => ({
      id: `profile-${index}`, agent_type, display_name: `Profile ${index}`, source: 'configured',
      available: index !== 4, command: `/opt/agent-${index}`, args: ['--profile', `profile ${index}`],
      env: { SESSION_HOME: `/sessions/profile-${index}` },
    }));
    const { page, state } = await fixture(t, { registeredAgents });
    state.sessionFixtures = ['codex', 'claude', 'traecli'].map(agent => ({
      id: `${agent}:native-id`, agent, session_id: `${agent}-native-id`, title: `${agent} history`,
      cwd: worktrees[1].path, created_at: task.created_at, updated_at: task.updated_at,
    }));
    await page.getByRole('button', { name: 'Conversation', exact: true }).click();
    const panel = surface(page).getByRole('region', { name: 'Conversation', exact: true });
    let createdCount = 0;
    for (const agent of ['codex', 'claude', 'traecli']) {
      const row = panel.locator('.project-aow-session-row').filter({ hasText: `${agent} history` });
      const profiles = registeredAgents.filter(profile => profile.agent_type === agent && profile.available);
      for (const profile of profiles) {
        if (createdCount % 2 === 0) await row.locator('.aow-list-row-open').click({ button: 'right' });
        else await row.getByRole('button', { name: `${agent} history 会话操作`, exact: true }).click();
        const menu = page.getByRole('menu', { name: `${agent} history 会话操作`, exact: true });
        assert.deepEqual(await menu.getByRole('menuitem').allTextContents(), [
          '打开', '打开 · 浮动工作区', ...profiles.map(profile => `Resume · ${profile.display_name}`),
        ]);
        await menu.dispatchEvent('scroll');
        assert.equal(await menu.isVisible(), true, 'scrolling the profile menu keeps it open');
        await menu.getByRole('menuitem', { name: `Resume · ${profile.display_name}`, exact: true }).click();
        createdCount += 1;
        await eventually(async () => state.createdTerminalTabs.length === createdCount);
        assert.deepEqual(state.createdTerminals.at(-1), {
          workspace_root: worktrees[0].path, cwd: worktrees[1].path,
          agent_id: profile.id, resume_session_id: `${agent}-native-id`,
        });
        await tab(page, profile.display_name).waitFor();
        assert.equal(await tab(page, profile.display_name).getAttribute('aria-selected'), 'true');
        assert.equal(await menu.count(), 0);
      }
    }
    assert.equal(state.requests.some(path => path.endsWith('/snapshot')), false, 'resuming does not open a history preview');
    assert.deepEqual(state.agentUpdates, [], 'profile settings are not changed');

    const row = panel.locator('.project-aow-session-row').filter({ hasText: 'codex history' });
    await row.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Resume · Profile 0', exact: true }).click();
    await eventually(async () => state.createdTerminalTabs.length === createdCount + 1);
    assert.equal(await tab(page, 'Profile 0').count(), 2, 'each resume creates a distinct tab');

    state.terminalCreateError = 'Resume launch failed';
    await row.getByRole('button', { name: 'codex history 会话操作', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Resume · Profile 1', exact: true }).click();
    await surface(page).getByText('Resume launch failed', { exact: true }).waitFor();
    assert.equal(state.createdTerminalTabs.length, createdCount + 1);
    assert.equal(await tab(page, 'Profile 1').count(), 1);
  });

  await test('TraeCode CLI registration exposes sessions only under the traecli identity', async t => {
    for (const id of ['traecli', 'traex']) {
      const { page, state } = await fixture(t, {
        registeredAgents: [{ id, display_name: 'TraeCode CLI', available: true, args: [], env: {} }],
      });
      await page.getByRole('button', { name: 'Conversation', exact: true }).click();
      const panel = surface(page).getByRole('region', { name: 'Conversation', exact: true });
      if (id === 'traecli') {
        const toggle = panel.getByRole('button', { name: /^(展开|收起) TraeCode CLI$/ });
        await eventually(async () => await toggle.getAttribute('aria-expanded') === 'false');
        assert.equal(await panel.locator('#agent-sessions-traecli').count(), 1);
        assert.equal(await panel.locator('#agent-sessions-traecli').isVisible(), false);
        assert.equal(await panel.getByRole('button', { name: '刷新 TraeCode CLI Sessions', exact: true }).count(), 1);
        assert.deepEqual(state.sessionAgents, ['traecli']);
        await toggle.click();
        await panel.getByText('暂无 Session', { exact: true }).waitFor();
        const refresh = panel.getByRole('button', { name: '刷新 TraeCode CLI Sessions', exact: true });
        await refresh.click();
        await eventually(async () => state.sessionAgents.length === 2 && !await refresh.isDisabled());
        assert.equal(await toggle.getAttribute('aria-expanded'), 'true', 'refresh preserves a manual expansion of an empty group');
      } else {
        await panel.getByText('暂无可用的 Session Agent', { exact: true }).waitFor();
        assert.equal(state.requests.includes('/api/aow/agent-sessions'), false);
      }
    }
  });

  async function openDiffs(page, fallback = false) {
    await page.getByRole('button', { name: 'Source Control', exact: true }).click();
    for (let i = 0; i < 3; i++) {
      await surface(page).locator(`.change-row[title="file-${i}.txt · working tree"]`).click();
      await surface(page).locator(fallback ? '.monaco-editor' : '.monaco-diff-editor').first().waitFor();
      await waitForDiff(page, i, fallback);
    }
    // Revisit loaded tabs: opening a file for the first time passes through a loading state.
    for (let i = 0; i < 3; i++) {
      await surface(page).locator(`.change-row[title="file-${i}.txt · working tree"]`).click();
      await waitForDiff(page, i, fallback);
    }
  }

  async function waitForDiff(page, index, fallback, documentId) {
    await eventually(() => page.evaluate(async ({ index, fallback, documentId }) => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      const matches = editor => {
        const path = editor.getModel()?.uri.toString();
        return path?.includes(`file-${index}.txt`) && (!documentId || decodeURIComponent(decodeURIComponent(path)).includes(documentId));
      };
      return fallback ? monaco.editor.getEditors().some(matches)
        : monaco.editor.getDiffEditors().some(diff => matches(diff.getModifiedEditor()) && diff.getLineChanges() !== null);
    }, { index, fallback, documentId }));
    // Let Monaco's 50 ms cursor-highlight task settle before the next model switch.
    await delay(100);
  }

  await test('Source Control polling is limited to the visible worktree and sidebar; Explorer never polls', async t => {
    const { page, state } = await fixture(t, { clock: true });
    const count = () => state.requests.filter(path => ['/api/git/status', '/api/git/log'].includes(path)).length;
    await page.clock.runFor(11000);
    assert.equal(count(), 0, 'Explorer does not start Git polling');
    const trees = state.requests.filter(path => path.startsWith('/api/fs/tree')).length;
    await page.getByRole('button', { name: 'Source Control', exact: true }).click();
    await eventually(() => count() === 2);
    await delay(100);
    await page.clock.runFor(5100);
    await eventually(() => count() === 4);
    assert.equal(state.requests.filter(path => path.startsWith('/api/fs/tree')).length, trees);
    await switchWorktree(page, 1);
    const before = count();
    await page.clock.runFor(11000);
    assert.equal(count(), before);
    await switchWorktree(page, 0);
    await eventually(() => count() === before + 2);
    await page.getByRole('button', { name: '隐藏右侧栏', exact: true }).click();
    const hidden = count();
    await page.clock.runFor(11000);
    assert.equal(count(), hidden);
  });

  const textPath = `${worktrees[0].path}/edit.txt`;
  async function textEditor(page, content) {
    return page.evaluate(async content => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      const editor = monaco.editor.getEditors().find(editor => editor.getModel()?.uri.toString().endsWith('/edit.txt'));
      if (!editor) return undefined;
      if (content !== undefined) editor.executeEdits('test', [{ range: editor.getModel().getFullModelRange(), text: content }]);
      return { value: editor.getValue(), modelId: editor.getModel().id, position: editor.getPosition() };
    }, content);
  }
  async function openText(page) {
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    await surface(page).locator(`.tree-row[data-tree-path="${textPath}"]`).click();
    await eventually(async () => (await textEditor(page))?.value === 'Initial text');
  }

  for (const action of ['close', 'switch']) {
    await test(`pending word highlights are cancelled cleanly when editors ${action}`, async t => {
      const otherPath = `${worktrees[0].path}/other.txt`;
      const { page, state } = await fixture(t, { beforeOpen: ({ state }) => {
        state.textFiles[otherPath] = { content: 'Other file', version: 'v1' };
      } });
      if (action === 'switch') {
        await page.getByRole('button', { name: 'Explorer', exact: true }).click();
        await surface(page).locator(`.tree-row[data-tree-path="${otherPath}"]`).click();
        await eventually(async () => (await modelPaths(page)).some(path => path.endsWith('/other.txt')));
      }
      await openText(page);
      const monaco = await page.evaluateHandle(async () => (await import('/src/features/editor/monaco.ts')).monaco);
      // Focus/move and close/switch within the same browser task, before Monaco's
      // 50 ms word-highlight timer can finish. Ordinary clicks can miss this race.
      await page.evaluate(({ monaco, action }) => {
        const area = document.querySelector('.project-aow-surface:not([hidden])');
        const editor = monaco.editor.getEditors().find(editor => editor.getDomNode()?.closest('.project-aow-surface') === area);
        editor.focus();
        editor.setPosition({ lineNumber: 1, column: 3 });
        if (action === 'close') area.querySelector('button[aria-label="关闭 edit.txt"]').click();
        else [...area.querySelectorAll('[role="tab"]')].find(tab => tab.textContent.includes('other.txt')).click();
      }, { monaco, action });
      await monaco.dispose();
      if (action === 'close') {
        await surface(page).getByRole('button', { name: '关闭 edit.txt', exact: true }).waitFor({ state: 'hidden' });
        await eventually(async () => !(await modelPaths(page)).some(path => path.endsWith('/edit.txt')));
      } else {
        await eventually(() => page.evaluate(async () => {
          const { monaco } = await import('/src/features/editor/monaco.ts');
          return monaco.editor.getEditors().some(editor => editor.getDomNode()?.checkVisibility() && editor.getValue() === 'Other file');
        }));
      }
      await delay(100);
      assert.deepEqual(state.errors, []);
      assert.deepEqual(state.writes, [], 'moving the cursor and leaving an editor must not change its file');
    });
  }

  await test('manual file refresh updates the existing model, reports deletion, and ignores a closed tab request', async t => {
    const { page, state } = await fixture(t, { clock: true });
    await openText(page);
    const original = await textEditor(page);
    const reads = () => state.requests.filter(path => path === `/api/fs/text${textPath}`).length;
    state.textFiles[textPath] = { content: 'External text', version: 'v2' };
    const before = reads();
    await page.clock.runFor(11000);
    assert.equal(reads(), before, 'ordinary files do not poll');
    assert.equal((await textEditor(page)).value, 'Initial text');
    await page.getByRole('button', { name: '刷新文件', exact: true }).click();
    await eventually(async () => (await textEditor(page)).value === 'External text');
    assert.equal((await textEditor(page)).modelId, original.modelId);
    assert.deepEqual((await textEditor(page)).position, original.position);
    state.failText = true;
    await page.getByRole('button', { name: '刷新文件', exact: true }).click();
    await page.locator('.editor-file-notice.error').filter({ hasText: 'File removed' }).waitFor();
    assert.equal((await textEditor(page)).value, 'External text');
    state.failText = false;
    let release;
    state.textGate = new Promise(resolve => { release = resolve; });
    try {
      const aborted = page.waitForEvent('requestfailed', request => request.url().includes(`/api/fs/text${textPath}`));
      await page.getByRole('button', { name: '刷新文件', exact: true }).click();
      assert.equal(await page.getByRole('button', { name: '刷新文件', exact: true }).isDisabled(), true);
      await page.getByRole('button', { name: '关闭 edit.txt', exact: true }).click();
      await aborted;
      state.textFiles[textPath] = { content: 'Reopened text', version: 'v3' };
      state.textGate = null;
      await surface(page).locator(`.tree-row[data-tree-path="${textPath}"]`).click();
      await eventually(async () => (await textEditor(page))?.value === 'Reopened text');
      release();
      await delay(100);
      assert.equal((await textEditor(page)).value, 'Reopened text');
      assert.notEqual((await textEditor(page)).modelId, original.modelId);
    } finally { release(); state.textGate = null; }
  });

  await test('image refresh preserves the preview on errors and releases replaced or closed Blob URLs', async t => {
    const { page, state } = await fixture(t);
    await surface(page).getByRole('button', { name: 'Explorer', exact: true }).click();
    const path = `${worktrees[0].path}/preview.svg`;
    state.textFiles[path] = { content: '', version: 'v1' };
    await page.getByRole('button', { name: '刷新 Project Explorer', exact: true }).click();
    await surface(page).locator(`.tree-row[data-tree-path="${path}"]`).click();
    const image = surface(page).locator('.image-preview img');
    await image.waitFor();
    await page.evaluate(() => {
      window.revokedPreviews = [];
      const revoke = URL.revokeObjectURL;
      URL.revokeObjectURL = url => { window.revokedPreviews.push(url); revoke(url); };
    });
    await page.getByRole('button', { name: '刷新文件', exact: true }).click();
    await eventually(async () => (await image.getAttribute('src')).startsWith('blob:'));
    const first = await image.getAttribute('src');
    state.failPreview = true;
    await page.getByRole('button', { name: '刷新文件', exact: true }).click();
    await page.locator('.editor-file-notice.error').waitFor();
    assert.equal(await image.getAttribute('src'), first);
    state.failPreview = false;
    state.previewColor = 'blue';
    await page.getByRole('button', { name: '刷新文件', exact: true }).click();
    await eventually(async () => (await image.getAttribute('src')) !== first);
    await eventually(() => image.evaluate(image => image.complete && image.naturalWidth === 32));
    assert.ok((await page.evaluate(() => window.revokedPreviews)).includes(first));
    const second = await image.getAttribute('src');
    await page.getByRole('button', { name: '关闭 preview.svg', exact: true }).click();
    await eventually(async () => (await page.evaluate(() => window.revokedPreviews)).includes(second));
  });

  await test('file refresh preserves edits made during the request and requires a choice for external changes', async t => {
    const { page, state } = await fixture(t, { clock: true });
    await openText(page);
    await page.clock.pauseAt('2026-09-15T01:00:00Z');
    state.textFiles[textPath] = { content: 'External change', version: 'v2' };
    let release;
    state.textGate = new Promise(resolve => { release = resolve; });
    try {
      await page.getByRole('button', { name: '刷新文件', exact: true }).dispatchEvent('click');
      await eventually(() => state.requests.filter(path => path === `/api/fs/text${textPath}`).length === 2);
      await textEditor(page, 'Unsaved typing');
      release(); state.textGate = null;
      await page.getByRole('button', { name: '从磁盘重新加载', exact: true }).waitFor();
      assert.equal((await textEditor(page)).value, 'Unsaved typing');
      await page.clock.runFor(1500);
      assert.deepEqual(state.writes, [], 'conflicted edits must not auto-save');
      await page.getByRole('button', { name: '从磁盘重新加载', exact: true }).dispatchEvent('click');
      await eventually(async () => (await textEditor(page)).value === 'External change');
      await textEditor(page, 'Keep these edits');
      await page.getByRole('button', { name: '刷新文件', exact: true }).dispatchEvent('click');
      await eventually(async () => !await page.getByRole('button', { name: '刷新文件', exact: true }).isDisabled());
      assert.equal((await textEditor(page)).value, 'Keep these edits', 'unchanged disk does not discard local edits');
      assert.equal(await page.locator('.editor-file-notice.warning').count(), 0);
      state.textFiles[textPath] = { content: 'Another external change', version: 'v3' };
      await page.getByRole('button', { name: '刷新文件', exact: true }).dispatchEvent('click');
      await page.getByRole('button', { name: '保留并覆盖', exact: true }).waitFor();
      page.once('dialog', dialog => dialog.accept());
      await page.getByRole('button', { name: '保留并覆盖', exact: true }).dispatchEvent('click');
      await page.clock.runFor(1000);
      await eventually(() => state.writes.length === 1);
      assert.deepEqual(state.writes[0], { path: textPath, content: 'Keep these edits', version: '"v3"' });
    } finally { release(); state.textGate = null; }
  });

  for (const fallback of [false, true]) {
    await test(`manual ${fallback ? 'patch' : 'two-sided'} refresh retains working, staged and commit diff sources`, async t => {
      const { page, state } = await fixture(t, { fallback });
      state.staged = true;
      state.commits = [{ id: 'commit-1', short_id: 'abc', subject: 'Fixture commit', author: 'Fixture', authored_at: '2026-09-15T00:00:00Z', parents: [], is_pushed: true }];
      await page.getByRole('button', { name: 'Source Control', exact: true }).click();
      for (const kind of ['working tree', 'staged', 'commit']) {
        if (kind === 'commit') {
          await page.locator('.commit-row').click();
          await page.locator('.commit-changes .change-row').click();
        } else await surface(page).locator(`.change-row[title="file-0.txt · ${kind}"]`).click();
        const documentId = kind === 'commit' ? `commit-diff:${worktrees[0].path}:commit-1:file-0.txt` : `diff:${worktrees[0].path}:${kind === 'staged'}:file-0.txt`;
        await waitForDiff(page, 0, fallback, documentId);
        const paths = await modelPaths(page);
        state.diffFiles[0] = { ...diffFiles[0], original: `${kind} old`, modified: `${kind} new`, patch: `${kind} patch` };
        let release;
        state.diffGate = new Promise(resolve => { release = resolve; });
        try {
          await page.getByRole('button', { name: '刷新 Diff', exact: true }).click();
          assert.equal(await page.getByRole('button', { name: '刷新 Diff', exact: true }).isDisabled(), true);
          release(); state.diffGate = null;
          await eventually(async () => !await page.getByRole('button', { name: '刷新 Diff', exact: true }).isDisabled());
          await waitForDiff(page, 0, fallback, documentId);
          const content = await page.evaluate(async ({ fallback, documentId }) => {
            const { monaco } = await import('/src/features/editor/monaco.ts');
            const matches = editor => decodeURIComponent(decodeURIComponent(editor.getModel()?.uri.toString() ?? '')).includes(documentId);
            if (fallback) return monaco.editor.getEditors().find(matches).getValue();
            const diff = monaco.editor.getDiffEditors().find(diff => matches(diff.getModifiedEditor()));
            return { original: diff.getOriginalEditor().getValue(), modified: diff.getModifiedEditor().getValue() };
          }, { fallback, documentId });
          assert.deepEqual(content, fallback ? `${kind} patch` : { original: `${kind} old`, modified: `${kind} new` });
          assert.deepEqual(await modelPaths(page), paths, 'refresh reuses models');
          const query = state.diffRequests.at(-1);
          assert.equal(query.repo, worktrees[0].path);
          assert.equal(query.path, 'file-0.txt');
          if (kind === 'commit') { assert.equal(query.commit, 'commit-1'); assert.equal(query.original_path, 'old.txt'); }
          else assert.equal(query.staged, String(kind === 'staged'));
          await delay(100);
        } finally { release(); state.diffGate = null; }
      }
    });
  }

  await test('Git diff wraps shared lines at the same columns on both sides', async t => {
    const shared = 'abcdefghijklmnopqrstuvwxyz '.repeat(10);
    const { page } = await fixture(t, { beforeOpen: async ({ state }) => {
      state.settings.editor = { word_wrap: true };
      state.diffFiles[0] = { ...diffFiles[0], original: `${shared}\nbefore\n${shared}`, modified: `${shared}\nafter\n${shared}` };
    } });
    await page.getByRole('button', { name: 'Source Control', exact: true }).click();
    await surface(page).locator('.change-row[title="file-0.txt · working tree"]').click();
    await waitForDiff(page, 0, false);

    const wrapping = () => page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      const diff = monaco.editor.getDiffEditors().find(diff => diff.getModifiedEditor().getDomNode()?.checkVisibility());
      return [diff.getOriginalEditor(), diff.getModifiedEditor()].map(editor => ({
        column: editor.getOption(monaco.editor.EditorOption.wrappingInfo).wrappingColumn,
        positions: Array.from({ length: editor.getModel().getLineLength(1) }, (_, index) =>
          editor.getScrolledVisiblePosition({ lineNumber: 1, column: index + 1 })?.top),
      }));
    });
    const assertWrapped = async () => {
      const [original, modified] = await wrapping();
      assert.ok(original.column > 0 && modified.column > 0, 'both panes must wrap long lines');
      assert.equal(original.column, modified.column, 'both panes must use the same wrapping column');
      assert.deepEqual(original.positions, modified.positions, 'unchanged text must break at the same characters');
    };
    await assertWrapped();
    // Cross Monaco's default inline breakpoint in both directions.
    for (const width of [1920, 1280, 1441]) {
      await page.setViewportSize({ width, height: 900 });
      await delay(250);
      await assertWrapped();
    }
    const wrap = page.getByRole('button', { name: '自动换行', exact: true });
    await wrap.click();
    await eventually(async () => (await wrapping()).every(side => side.column === -1));
    await wrap.click();
    await eventually(async () => (await wrapping()).every(side => side.column > 0));
    await assertWrapped();
  });

  await test('switching loaded diff tabs keeps the displayed files and computed hunks in sync', async t => {
    const { page } = await fixture(t);
    await openDiffs(page);

    async function assertDiff(index) {
      const actual = await page.evaluate(async index => {
        const { monaco } = await import('/src/features/editor/monaco.ts');
        const diff = monaco.editor.getDiffEditors().find(diff => diff.getModifiedEditor().getModel()?.uri.toString().includes(`file-${index}.txt`));
        const model = diff.getModel();
        return {
          originalMatches: model.original === diff.getOriginalEditor().getModel(),
          modifiedMatches: model.modified === diff.getModifiedEditor().getModel(),
          original: diff.getOriginalEditor().getValue(),
          modified: diff.getModifiedEditor().getValue(),
          changes: diff.getLineChanges().map(({ charChanges, ...lines }) => lines),
          cached: monaco.editor.getModels().map(model => ({ path: model.uri.toString(), content: model.getValue() })),
        };
      }, index);
      assert.ok(actual.originalMatches && actual.modifiedMatches, 'diff computation must use the same models as the visible editors');
      assert.equal(actual.original, diffFiles[index].original);
      assert.equal(actual.modified, diffFiles[index].modified);
      const line = diffFiles[index].changedLine;
      assert.deepEqual(actual.changes, [{ originalStartLineNumber: line, originalEndLineNumber: line, modifiedStartLineNumber: line, modifiedEndLineNumber: line }]);
      for (const { path, content } of actual.cached) {
        const file = diffFiles[Number(path.match(/file-(\d+)/)[1])];
        assert.equal(content, path.includes('/original/') ? file.original : file.modified, `cached content was changed for ${path}`);
      }
    }

    for (const index of [0, 1, 2, 1, 0]) {
      await tab(page, `file-${index}.txt (Diff)`).click();
      await waitForDiff(page, index, false);
      await assertDiff(index);
    }
    await tab(page, 'Shell').click();
    await tab(page, 'file-0.txt (Diff)').click();
    await page.setViewportSize({ width: 1280, height: 800 });
    // Check again after asynchronous diff calculation and layout updates settle.
    await delay(1000);
    await assertDiff(0);
  });

  for (const fallback of [false, true]) {
    await test(`${fallback ? 'patch' : 'two-sided'} diff models are released by single and group closes`, async t => {
      const { page } = await fixture(t, { fallback });
      const perFile = fallback ? 1 : 2;
      await openDiffs(page, fallback);
      assert.equal((await modelPaths(page)).length, 3 * perFile);
      await page.getByRole('button', { name: '关闭 file-0.txt (Diff)', exact: true }).click();
      await eventually(async () => (await modelPaths(page)).length === 2 * perFile);
      assert.ok((await modelPaths(page)).every(path => !path.includes('file-0.txt')));
      await tab(page, 'file-2.txt (Diff)').click({ button: 'right' });
      await page.getByRole('menuitem', { name: '关闭其他', exact: true }).click();
      await eventually(async () => (await modelPaths(page)).length === perFile);
      assert.ok((await modelPaths(page)).every(path => path.includes('file-2.txt')));
      await openDiffs(page, fallback);
      await tab(page, 'file-2.txt (Diff)').click({ button: 'right' });
      await page.getByRole('menuitem', { name: '关闭所有', exact: true }).click();
      await eventually(async () => (await modelPaths(page)).length === 0);
      assert.equal(await surface(page).locator('.xterm').count(), 1, 'closing diffs preserves the terminal');
    });
  }

  await test('worktree switching keeps terminals and diff models; unregistering releases the models', async t => {
    const { page, state } = await fixture(t);
    await openDiffs(page);
    await switchWorktree(page, 1);
    await eventually(async () => await page.locator('.xterm').count() === 2);
    assert.equal((await modelPaths(page)).length, 6, 'hidden worktree retains its open diffs');
    state.projects = [];
    await page.getByRole('button', { name: '刷新全部', exact: true }).click();
    await eventually(async () => await page.locator('.project-aow-surface').count() === 0);
    await eventually(async () => (await modelPaths(page)).length === 0);
  });

  await test('automation polling pauses for hidden tabs, worktrees and browser pages, then resumes without losing history state', async t => {
    const { page, state } = await fixture(t, { clock: true });
    const counts = () => [detailPath, runsPath].map(path => state.requests.filter(request => request === path).length);
    await page.getByRole('button', { name: 'Automation', exact: true }).click();
    await page.getByRole('listitem').filter({ hasText: task.name }).click();
    await page.getByRole('heading', { name: task.name, exact: true }).waitFor();
    await page.getByRole('button', { name: '执行历史', exact: true }).click();
    await page.locator('.automation-run-summary').click();
    await page.locator('.automation-run-detail').waitFor();
    await page.clock.pauseAt('2026-09-15T01:00:00Z');
    await delay(100);

    const advance = async () => { await page.clock.runFor(6500); await delay(100); };
    const initial = counts();
    await advance();
    assert.ok(counts().every((count, i) => count > initial[i]), 'visible detail and history both poll');

    const assertPaused = async () => {
      await delay(100);
      const before = counts();
      await advance();
      assert.deepEqual(counts(), before);
    };
    const assertResumed = async show => {
      const before = counts();
      await show();
      await eventually(() => counts().every((count, i) => count > before[i]));
      assert.equal(await surface(page).locator('.automation-run-summary').getAttribute('aria-expanded'), 'true');
    };
    await tab(page, 'Shell').dispatchEvent('click');
    await assertPaused();
    await assertResumed(() => tab(page, task.name).dispatchEvent('click'));

    await switchWorktree(page, 1);
    await assertPaused();
    assert.equal(await page.locator('.xterm').count(), 2, 'background terminals remain mounted');
    await assertResumed(() => switchWorktree(page, 0));

    const setHidden = hidden => page.evaluate(hidden => {
      Object.defineProperty(document, 'hidden', { configurable: true, value: hidden });
      Object.defineProperty(document, 'visibilityState', { configurable: true, value: hidden ? 'hidden' : 'visible' });
      document.dispatchEvent(new Event('visibilitychange'));
    }, hidden);
    await setHidden(true);
    await assertPaused();
    await assertResumed(() => setHidden(false));

    // A delayed poll must neither pile up requests nor update a hidden detail after it completes.
    let release;
    state.detailGate = new Promise(resolve => { release = resolve; });
    state.task = { ...task, name: 'Stale response' };
    const beforeSlow = counts()[0];
    await advance();
    assert.equal(counts()[0], beforeSlow + 1);
    await tab(page, 'Shell').dispatchEvent('click');
    await delay(100);
    release();
    state.detailGate = null;
    state.task = { ...task };
    await delay(100);
    assert.equal(await page.getByRole('heading', { name: 'Stale response', includeHidden: true }).count(), 0);
    await assertResumed(() => tab(page, task.name).dispatchEvent('click'));
  });
} finally {
  await browser?.close();
  await server.close();
}
