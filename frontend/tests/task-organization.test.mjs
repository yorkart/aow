import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';
import { acpControlFixture } from './fixtures/acp-control.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('tasks reuse workspace panels and keep independent resources in a shared worktree', async t => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    t.after(() => context.close());
    await context.addInitScript(() => localStorage.setItem('aow.language', 'en-US'));
    let failDelete = false, nextTerminal = 0, routerCreations = 0;
    const tasks = [], sessions = [], terminals = [], creations = [], deleted = [], worktreeRequests = [];
    const worktree = { id: 'wt', project_id: 'project', path: '/repo', branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' };
    const project = { id: 'project', name: 'Task project', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes', worktrees: [worktree] };
    const agent = { id: 'codex', agent_type: 'codex', display_name: 'Terminal Codex', available: true, source: 'configured', command: 'codex', args: [], env: {} };
    const role = { id: 'cli-role', name: 'CLI reviewer', runtime: { mode: 'cli', agent_id: 'codex' }, home_layout: 'codex' };
    function session(id, cwd = '/repo') {
      const value = { thread_id: id, session_id: `remote-${id}`, agent_id: 'test', cwd, title: id, source: 'user', parent: null, status: 'idle', revision: 1, entries: [], permissions: [], modes: {}, config_options: [], commands: [], usage: null, created_at: '2026-10-31T00:00:00Z', updated_at: '2026-10-31T00:00:00Z' };
      sessions.push(value); return value;
    }
    await installLiveEvents(context, undefined, () => ({ tabs: terminals, metadata: { agents: {}, titles: {}, processes: {}, activity: {} }, error: null, agent_error: null }));
    await acpControlFixture().install(context);
    await context.routeWebSocket('**/api/terminals/*/panes/*/ws*', socket => {
      socket.onMessage(raw => {
        if (typeof raw === 'string' && JSON.parse(raw).type === 'claim') {
          socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
          socket.send(JSON.stringify({ type: 'stream', epoch: 'fixture', offset: 0, reset: true, replay_bytes: 0, restore_cols: 80, restore_rows: 24, restore: '$ ' }));
        }
      });
    });
    await context.route('**/api/**', async route => {
      const request = route.request(), url = new URL(request.url()), path = url.pathname;
      let data = [];
      if (path === '/api/aow/projects') data = [project];
      else if (path === '/api/aow/agents') data = [agent];
      else if (path === '/api/aow/agent-roles') data = [role];
      else if (path === '/api/aow/settings') data = { notes_base: '/notes', execution_path: [], editor: {} };
      else if (path.includes('/pinned-')) data = { paths: [], revision: 0 };
      else if (path === '/api/aow/notifications') data = { revision: 0, items: [], unread_count: 0 };
      else if (path === '/api/instances') data = { items: [] };
      else if (path === '/api/aow/projects/project/router' && request.method() === 'POST') {
        routerCreations++;
        data = tasks.find(task => task.is_router);
        if (!data) {
          const first = session('router-thread');
          data = { id: 'router-project', project_id: 'project', prompt: 'Router', agent_id: 'test', is_router: true, workspace: '/repo', workspace_mode: 'existing', dependencies: [], pinned: false, archived: false, initial_thread_id: first.thread_id, resources: [{ kind: 'acp', id: first.thread_id }] };
          tasks.push(data);
        }
      }
      else if (path === '/api/aow/tasks') {
        if (request.method() === 'POST') {
          const input = request.postDataJSON(); creations.push(input);
          const workspace = input.workspace_mode === 'new_worktree' ? '/repo-isolated' : input.workspace;
          if (input.workspace_mode === 'new_worktree') project.worktrees.push({ ...worktree, id: 'new-wt', path: workspace, branch: `task/${input.request_id}-isolated-implementation`, is_main: false });
          if (input.agent_role_id) {
            const id = `role-terminal-${tasks.length}`;
            terminals.push({ id, name: role.name, workspace_root: workspace, layout: { type: 'pane', pane_id: id }, panes: [{ id, name: role.name, cwd: workspace, kind: 'agent', agent_id: 'codex', agent_profile_id: `role:${role.id}`, shell: 'codex', status: 'running', rows: 24, cols: 80, agent_terminal: { phase: 'ready', task_submitted: true } }] });
            data = { ...input, workspace, slug: null, id: input.request_id, initial_thread_id: null, pinned: false, archived: false, resources: [{ kind: 'terminal', id }] };
          } else {
            const first = session(`initial-${tasks.length}`, workspace);
            data = { ...input, workspace, slug: null, id: input.request_id, initial_thread_id: first.thread_id, pinned: false, archived: false, resources: [{ kind: 'acp', id: first.thread_id }] };
          }
          tasks.push(data);
        } else data = tasks;
      } else if (path.startsWith('/api/aow/tasks/') && request.method() === 'GET') {
        data = { task: tasks.find(task => task.id === path.split('/').at(-1)), runtime: { status: 'idle', response: 'Finished investigation' }, delegated: [] };
      } else if (path.startsWith('/api/aow/tasks/') && request.method() === 'PATCH') {
        Object.assign(tasks.find(task => task.id === path.split('/').at(-1)), request.postDataJSON());
        return route.fulfill({ status: 204 });
      } else if (path.endsWith('/resources')) {
        const task = tasks.find(task => task.id === path.split('/').at(-2));
        task.resources.push(request.postDataJSON()); return route.fulfill({ status: 204 });
      } else if (path === '/api/acp/agents') data = [{ id: 'test', name: 'Test Agent', configured: true, supported: true, installed: true }];
      else if (path === '/api/acp/settings') data = { content: '{}', revision: '1' };
      else if (path === '/api/acp/sessions') {
        if (request.method() === 'POST') {
          data = session(`helper-${sessions.length}`);
          tasks.find(task => task.id === request.headers()['x-aow-task']).resources.push({ kind: 'acp', id: data.thread_id });
        } else data = sessions.filter(session => session.cwd === url.searchParams.get('cwd'));
      } else if (path.startsWith('/api/acp/sessions/')) {
        if (path.endsWith('/connection')) data = null;
        else if (path.endsWith('/requests')) data = [];
        else data = sessions.find(session => session.thread_id === path.split('/')[4]);
      } else if (path === '/api/terminals' && request.method() === 'POST') {
        const input = request.postDataJSON(), id = `terminal-${nextTerminal++}`;
        data = { id, name: input.agent_id ? 'Terminal agent' : 'Shell', workspace_root: input.workspace_root, layout: { type: 'pane', pane_id: id }, panes: [{ id, name: 'Shell', cwd: input.cwd, kind: input.agent_id ? 'agent' : 'terminal', agent_id: input.agent_id, shell: '/bin/bash', status: 'running', rows: 24, cols: 80 }] };
        terminals.push(data);
        tasks.find(task => task.id === request.headers()['x-aow-task']).resources.push({ kind: 'terminal', id });
      } else if (path.startsWith('/api/terminals/') && request.method() === 'DELETE') { if (failDelete) return route.fulfill({ status: 503, json: { error: 'Cannot stop terminal' } }); deleted.push(path); const index = terminals.findIndex(tab => tab.id === path.split('/')[3]); if (index >= 0) terminals.splice(index, 1); return route.fulfill({ status: 204 }); }
      else if (path === '/api/terminals') data = terminals;
      else if (path === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
      else if (path === '/api/aow/projects/project/worktrees' && request.method() === 'POST') {
        worktreeRequests.push(request.postDataJSON());
      } else if (path === '/api/aow/worktree-creations') data = [];
      else if (path.startsWith('/api/fs/tree')) data = { path: '/repo', entries: [] };
      else if (path === '/api/git/ignored') data = { repository: '/repo', ignored: [] };
      else if (path === '/api/git/status') data = { repository: '/repo', branch: 'main', files: [] };
      else if (path === '/api/git/log') data = { commits: [], repository: '/repo', upstream: null };
      else if (path === '/api/operations') data = { boot_id: 'fixture', revision: 0, operations: [] };
      return route.fulfill({ json: data });
    });
    const page = await context.newPage(); page.setDefaultTimeout(15000);
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/task-organization-preview.html`);
    const semantics = await page.evaluate(async () => {
      const { taskResourceOwners } = await import('/src/features/tasks/membership.ts');
      const { parseTabUrl, tabTargetUrl } = await import('/src/aow/tabRoutes/index.ts');
      const owners = taskResourceOwners([
        { id: 'a', resources: [{ kind: 'acp', id: 'root' }] },
        { id: 'b', resources: [{ kind: 'acp', id: 'separate' }] },
      ], [
        { kind: 'acp', id: 'nested', parent: { kind: 'terminal', id: 'pane' } },
        { kind: 'terminal', id: 'pane', tab_id: 'shell', parent: { kind: 'acp', id: 'root' } },
        { kind: 'acp', id: 'separate', parent: { kind: 'acp', id: 'root' } },
      ]);
      const target = { type: 'file', workspace: 'wt', path: '/repo/readme.md', source: 'project', workTaskId: 'a' };
      const restored = parseTabUrl(new URL(tabTargetUrl(target), window.location.origin));
      return { owners: [...owners], restored };
    });
    assert.equal(new Map(semantics.owners).get('acp:nested'), 'a');
    assert.equal(new Map(semantics.owners).get('terminal:shell'), 'a');
    assert.equal(new Map(semantics.owners).get('acp:separate'), 'b');
    assert.equal(semantics.restored.workTaskId, 'a');
    await page.getByRole('button', { name: 'Organize by task', exact: true }).click();
    const surface = page.locator('.project-aow-surface:visible');
    const sidebar = page.locator('#aow-project-sidebar');
    const row = name => sidebar.locator('.project-aow-task-row').filter({ has: page.locator('.aow-list-row-title', { hasText: name }) }).last();
    const menu = async (name, action) => {
      await row(name).getByRole('button', { name: `${name} Task actions`, exact: true }).click();
      await page.getByRole('menuitem', { name: action, exact: true }).click();
    };
    const create = async (prompt, initialAgent) => {
      await sidebar.getByRole('button', { name: 'New task', exact: true }).click();
      await page.getByLabel('Task description', { exact: true }).fill(prompt);
      assert.equal(await page.locator('dialog.task-dialog input[type=checkbox]').count(), 0);
      if (initialAgent) {
        await page.getByRole('button', { name: 'Initial agent', exact: true }).click();
        await page.getByRole('option', { name: initialAgent, exact: true }).click();
      }
      await page.getByRole('button', { name: 'Create task', exact: true }).click();
      await row(prompt).waitFor();
      await surface.getByRole('tab').first().waitFor();
    };
    await create('Implement login');
    assert.equal(await surface.getByRole('button', { name: 'ACP panel', exact: true }).count(), 0);
    assert.equal(await surface.getByRole('button', { name: 'Terminal panel', exact: true }).count(), 0);
    assert.equal(await surface.getByRole('button', { name: 'Conversation', exact: true }).count(), 1);
    assert.equal(await surface.getByRole('button', { name: 'Automation', exact: true }).count(), 1);
    const first = surface.getByRole('tab').filter({ hasText: 'initial-0' });
    assert.equal(await first.locator('.project-aow-tab-close').count(), 1);
    await surface.locator('.project-aow-new-trigger').click();
    await page.locator('.project-aow-new-menu').getByRole('button', { name: /New.*Test Agent.*ACP|New.*ACP.*Test Agent/ }).click();
    await surface.getByRole('tab').filter({ hasText: 'helper-' }).waitFor();
    assert.equal(tasks.length, 1); assert.equal(tasks[0].resources.length, 2);
    await surface.locator('.project-aow-new-trigger').click();
    await page.locator('.project-aow-new-menu').getByRole('button', { name: 'Terminal', exact: true }).click();
    await surface.getByRole('tab').filter({ hasText: 'Shell' }).waitFor();
    await surface.locator('.project-aow-new-trigger').click();
    await page.locator('.project-aow-new-menu').getByRole('button', { name: 'Terminal Codex', exact: true }).click();
    await surface.getByRole('tab').filter({ hasText: 'Terminal agent' }).waitFor();
    assert.equal(tasks[0].resources.length, 4);
    const helper = surface.getByRole('tab').filter({ hasText: 'helper-' });
    await helper.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Open · Floating workspace', exact: true }).click();
    const floating = page.getByRole('dialog', { name: 'Floating workspace', exact: true });
    await floating.getByRole('tab').filter({ hasText: 'helper-' }).waitFor();
    assert.equal((await page.evaluate(() => JSON.parse(localStorage.getItem('aow-floating-tabs-v1'))))[0].workspace, `task:${tasks[0].id}`);
    await floating.getByRole('button', { name: 'Minimize floating workspace', exact: true }).click();
    await surface.getByRole('button', { name: 'Move to current workspace', exact: true }).click();
    assert.equal(tasks[0].resources.length, 4, 'moving a tab preserves the existing resource');
    await first.locator('.project-aow-tab-close').click();
    await create('Integrate page');
    assert.deepEqual(creations[1].dependencies, []);
    assert.equal(creations[1].parent_id, undefined);
    assert.equal(await surface.getByRole('tab').count(), 1);
    await surface.getByRole('button', { name: 'Conversation', exact: true }).click();
    await surface.locator('.conversation-instance').filter({ hasText: 'initial-1' }).waitFor();
    assert.equal(await surface.locator('.conversation-instance').count(), 1);
    await row('Implement login').getByRole('button', { name: 'Implement login', exact: true }).click();
    await surface.getByRole('tab').filter({ hasText: 'helper-' }).waitFor();
    assert.equal(await surface.getByRole('tab').filter({ hasText: 'initial-0' }).count(), 0);
    await surface.getByRole('tab').filter({ hasText: 'Shell' }).locator('.project-aow-tab-close').click();
    await surface.getByRole('tab').filter({ hasText: 'Shell' }).waitFor({ state: 'hidden' });
    assert.deepEqual(deleted, ['/api/terminals/terminal-0']);
    await menu('Implement login', 'Pin task');
    await sidebar.locator('.project-aow-pinned-panel').getByRole('button', { name: 'Implement login', exact: true }).waitFor();
    await page.reload();
    await surface.getByRole('tab').filter({ hasText: 'helper-' }).waitFor();
    assert.equal(await surface.getByRole('tab').filter({ hasText: 'initial-0' }).count(), 0);
    assert.equal(await surface.getByRole('tab').filter({ hasText: 'Shell' }).count(), 0);
    await menu('Implement login', 'Archive task');
    await row('Implement login').waitFor({ state: 'hidden' });
    await surface.getByRole('tab').filter({ hasText: 'initial-1' }).waitFor();
    assert.equal(tasks[1].archived, false); assert.deepEqual(deleted, ['/api/terminals/terminal-0']);
    await sidebar.getByRole('button', { name: 'New task', exact: true }).click();
    await page.getByLabel('Task description', { exact: true }).fill('Isolated implementation');
    await page.getByRole('button', { name: 'Workspace', exact: true }).click();
    await page.getByRole('textbox', { name: 'Search worktrees…', exact: true }).fill('no-match');
    await page.getByText('No matches', { exact: true }).waitFor();
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('dialog.task-dialog[open]').count(), 1, 'Escape first closes the picker');
    await page.getByRole('button', { name: 'Workspace', exact: true }).click();
    await page.getByRole('textbox', { name: 'Search worktrees…', exact: true }).fill('New isolated');
    await page.keyboard.press('Enter');
    assert.equal(creations.length, 2, 'picker Enter must not submit the task');
    assert.equal(await page.locator('.task-context-workspace').innerText(), 'Workspace · New isolated worktree');
    assert.equal(await page.getByLabel('Branch', { exact: true }).count(), 0);
    assert.equal(await page.getByLabel('Worktree path', { exact: true }).count(), 0);
    await page.getByRole('button', { name: 'Initial agent', exact: true }).click();
    await page.getByRole('option', { name: 'Test Agent ACP', exact: true }).click();
    await page.screenshot({ path: '/tmp/aow-task-dialog.png' });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: '/tmp/aow-task-dialog-mobile.png' });
    const bounds = await page.locator('dialog.task-dialog').boundingBox();
    assert.ok(bounds.x >= 0 && bounds.x + bounds.width <= 390);
    await page.setViewportSize({ width: 1440, height: 1000 });
    await page.getByLabel('Task description', { exact: true }).press('Control+Enter');
    await row('Isolated implementation').waitFor();
    await surface.getByRole('tab').filter({ hasText: 'initial-2' }).waitFor();
    assert.deepEqual(worktreeRequests, [], 'worktree provisioning belongs to the task API');
    assert.equal(creations.at(-1).workspace_mode, 'new_worktree');
    for (const field of ['workspace', 'branch', 'base_ref', 'path', 'slug']) assert.equal(creations.at(-1)[field], undefined);
    assert.equal(tasks.at(-1).workspace, '/repo-isolated');
    tasks[2].dependencies = [tasks[1].id];
    await page.reload();
    await row('Isolated implementation').waitFor();
    await menu('Isolated implementation', 'Edit task');
    await page.getByLabel('Task description', { exact: true }).fill('Review isolated implementation');
    assert.equal(await page.locator('dialog.task-dialog input[type=checkbox]').count(), 0);
    await page.getByRole('button', { name: 'Save', exact: true }).click();
    await row('Review isolated implementation').waitFor();
    assert.equal(creations.length, 3, 'editing must not create another task or ACP');
    assert.deepEqual(tasks[2].dependencies, [tasks[1].id]);
    await page.getByRole('button', { name: 'Organize by worktree', exact: true }).click();
    await sidebar.locator('.project-aow-worktrees').waitFor();
    await page.getByRole('button', { name: 'Organize by task', exact: true }).click();
    await create('Run CLI role', 'CLI reviewer CLI');
    const cli = surface.getByRole('tab').filter({ hasText: role.name });
    await cli.waitFor();
    assert.equal(await cli.getAttribute('aria-selected'), 'true');
    assert.equal(await surface.getByRole('tab').count(), 1);
    await page.reload();
    await cli.waitFor();
    assert.equal(await cli.getAttribute('aria-selected'), 'true');
    failDelete = true;
    await cli.locator('.project-aow-tab-close').click();
    await surface.getByRole('alert').filter({ hasText: /Cannot stop terminal|503/ }).waitFor();
    assert.equal(await cli.isVisible(), true, 'failed destruction must keep the tab accessible');
    failDelete = false;
    await cli.locator('.project-aow-tab-close').click();
    await cli.waitFor({ state: 'hidden' });
    await page.reload();
    await row('Run CLI role').waitFor();
    await page.waitForFunction(taskId => JSON.parse(localStorage.getItem(`aow-workspace-tabs:task:${taskId}`))?.terminalVisibility, tasks.at(-1).id);
    assert.equal(await cli.count(), 0, 'a closed initial terminal must stay closed after reloading');
    const createTerminal = async () => {
      await surface.locator('.project-aow-new-trigger').click();
      await page.locator('.project-aow-new-menu').getByRole('button', { name: 'Terminal Codex', exact: true }).click();
    };
    await createTerminal();
    const agentTab = surface.getByRole('tab').filter({ hasText: 'Terminal agent' });
    await agentTab.waitFor();
    const floatingId = terminals.at(-1).id;
    await agentTab.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Open · Floating workspace', exact: true }).click();
    await floating.getByRole('tab').filter({ hasText: 'Terminal agent' }).waitFor();
    assert.equal(deleted.includes(`/api/terminals/${floatingId}`), false, 'moving preserves the process');
    await floating.getByRole('tab').filter({ hasText: 'Terminal agent' }).locator('.project-aow-tab-close').click();
    await floating.getByRole('tab').filter({ hasText: 'Terminal agent' }).waitFor({ state: 'hidden' });
    assert.equal(deleted.includes(`/api/terminals/${floatingId}`), true, 'floating close destroys the process');
    await createTerminal();
    await createTerminal();
    await agentTab.first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Close all', exact: true }).click();
    await agentTab.first().waitFor({ state: 'hidden' });
    assert.equal(terminals.filter(tab => tasks.at(-1).resources.some(resource => resource.kind === 'terminal' && resource.id === tab.id)).length, 0);
    if (await floating.isVisible()) await floating.getByRole('button', { name: 'Minimize floating workspace', exact: true }).click();
    await sidebar.getByRole('button', { name: 'Open project Router', exact: true }).click();
    const routerDialog = page.getByRole('dialog');
    await routerDialog.getByText(/Choose an agent for the project Router/).waitFor();
    assert.equal(await routerDialog.getByRole('textbox', { name: 'Task description', exact: true }).count(), 0);
    await routerDialog.getByRole('button', { name: 'Open project Router', exact: true }).click();
    await row('Router').waitFor();
    await surface.getByRole('tab').filter({ hasText: 'router-thread' }).waitFor();
    await surface.locator('.task-collaboration summary').filter({ hasText: 'Project tasks' }).waitFor();
    await surface.locator('.task-collaboration-card').first().getByRole('button', { name: 'Check result', exact: true }).click();
    await surface.getByText('Finished investigation', { exact: true }).waitFor();
    await page.reload();
    await row('Router').waitFor();
    await sidebar.getByRole('button', { name: 'Open project Router', exact: true }).click();
    assert.equal(routerCreations, 1, 'opening the persistent Router reuses its task and conversation');
    assert.equal(await page.getByRole('dialog').count(), 0);
    tasks[2].source_task_id = tasks[1].id;
    await row(tasks[1].prompt).getByRole('button', { name: tasks[1].prompt, exact: true }).click();
    await page.reload();
    await surface.locator('.task-collaboration summary').filter({ hasText: 'Delegated tasks' }).waitFor();
    await surface.locator('.task-collaboration-title').filter({ hasText: tasks[2].prompt }).click();
    await surface.locator('.task-collaboration').getByText('Requested by:', { exact: false }).waitFor();
    // A delegated task may appear before the project/worktree event reaches this browser.
    const delegated = session('delegated-thread', '/repo-delegated');
    project.worktrees.push({ ...worktree, id: 'delegated-wt', path: delegated.cwd, branch: 'task/delegated', is_main: false });
    tasks.push({ ...tasks[2], id: 'delegated', prompt: 'Delegated implementation', workspace: delegated.cwd, initial_thread_id: delegated.thread_id, dependencies: [], resources: [{ kind: 'acp', id: delegated.thread_id }] });
    await page.evaluate(() => window.dispatchEvent(new Event('aow:tasks-changed')));
    await row('Delegated implementation').getByRole('button', { name: 'Delegated implementation', exact: true }).click();
    await surface.getByRole('tab').filter({ hasText: delegated.thread_id }).waitFor();
    assert.deepEqual(errors, []);
    await page.screenshot({ path: '/tmp/aow-task-organization.png' });
  });
} finally { await browser?.close(); await server.close(); }
