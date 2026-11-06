import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';
import { detail, pullRequests } from './fixtures/pull-request.mjs';
import { acpControlFixture } from './fixtures/acp-control.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('Agent workspace keeps conversation central and tools scoped to each Agent worktree', async t => {
    const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
    t.after(() => context.close());
    await context.addInitScript(() => localStorage.setItem('aow.language', 'en-US'));
    const failStatus = false, failDelete = false; let nextTerminal = 0;
    const createdAt = Date.parse('2026-11-05T09:00:00Z');
    let updatedAt = createdAt;
    const tasks = [], sessions = [], terminals = [], creations = [], deleted = [], worktreeRequests = [];
    const worktree = { id: 'wt', project_id: 'project', path: '/repo', branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' };
    const toolRequests = [], writes = [], searchRequests = [];
    let names = ['a.md', 'b.md', 'c.md', 'slow.md'];
    let slowRead, releaseSlow;
    t.after(() => releaseSlow?.());
    const project = { id: 'project', name: 'Task project', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes', worktrees: [worktree, { ...worktree, id: 'wt-other', path: '/repo-other', branch: 'other', is_main: false }] };
    const agent = { id: 'codex', agent_type: 'codex', display_name: 'Terminal Codex', available: true, source: 'configured', command: 'codex', args: [], env: {} };
    const role = { id: 'reviewer', name: 'Reviewer', runtime: { mode: 'acp', agent_id: 'test' }, home_layout: 'codex' };
    const cliRole = { ...role, id: 'cli-role', name: 'CLI reviewer', runtime: { mode: 'cli', agent_id: 'codex' } };
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
      if (path === '/api/projects') data = [project];
      else if (path === '/api/agents') data = [agent];
      else if (path === '/api/agent-roles') data = [role, cliRole];
      else if (path === '/api/settings') data = { notes_base: '/notes', execution_path: [], editor: {} };
      else if (path.includes('/pinned-')) data = { paths: [], revision: 0 };
      else if (path === '/api/notifications') data = { revision: 0, items: [], unread_count: 0 };
      else if (path === '/api/instances') data = { items: [] };
      else if (path === '/api/projects/project/router' && request.method() === 'POST') {
        data = tasks.find(task => task.is_router);
        if (!data) {
          const first = session('router-thread');
          data = { status: 'todo', created_at: createdAt, updated_at: createdAt, completed_at: null, id: 'router-project', project_id: 'project', prompt: 'Router', agent_id: 'test', is_router: true, workspace: '/repo', workspace_mode: 'existing', dependencies: [], pinned: false, archived: false, initial_thread_id: first.thread_id, resources: [{ kind: 'acp', id: first.thread_id }] };
          tasks.push(data);
        }
      }
      else if (path === '/api/tasks') {
        if (request.method() === 'POST') {
          const input = request.postDataJSON(); creations.push(input);
          const workspace = input.workspace_mode === 'new_worktree' ? '/repo-isolated' : input.workspace;
          if (input.workspace_mode === 'new_worktree') project.worktrees.push({ ...worktree, id: 'new-wt', path: workspace, branch: `task/${input.request_id}-isolated-implementation`, is_main: false });
          const first = session(`initial-${tasks.length}`, workspace);
          data = { ...input, created_at: createdAt, updated_at: createdAt, completed_at: null, status: 'in_progress', workspace, slug: null, id: input.request_id, initial_thread_id: first.thread_id, pinned: false, archived: false, resources: [{ kind: 'acp', id: first.thread_id }] };
          tasks.push(data);
        } else data = tasks;
      } else if (path.startsWith('/api/tasks/') && request.method() === 'GET') {
        const task = tasks.find(task => task.id === path.split('/').at(-1));
        data = { task, conversations: sessions.filter(session => task.resources.some(resource => resource.kind === 'acp' && resource.id === session.thread_id)), delegated: [] };
      } else if (path.startsWith('/api/tasks/') && path.endsWith('/status') && request.method() === 'POST') {
        if (failStatus) return route.fulfill({ status: 500, json: { error: 'Status could not be saved' } });
        data = tasks.find(task => task.id === path.split('/').at(-2));
        Object.assign(data, request.postDataJSON(), { updated_at: updatedAt += 60_000 });
        data.completed_at = data.status === 'done' ? data.updated_at : null;
      } else if (path.startsWith('/api/tasks/') && request.method() === 'PATCH') {
        Object.assign(tasks.find(task => task.id === path.split('/').at(-1)), request.postDataJSON());
        return route.fulfill({ status: 204 });
      } else if (path.endsWith('/resources')) {
        const task = tasks.find(task => task.id === path.split('/').at(-2));
        task.resources.push(request.postDataJSON()); return route.fulfill({ status: 204 });
      } else if (path === '/api/acp/agents') data = [{ id: 'test', name: 'Test Agent', configured: true, supported: true, installed: true }];
      else if (path === '/api/agent-roles/reviewer/sessions' && request.method() === 'POST') {
        const input = request.postDataJSON(), task = tasks.find(task => task.id === request.headers()['x-aow-task']);
        const created = session(`helper-${sessions.length}`, '/repo-other');
        created.parent = { kind: 'acp', id: input.parent_thread_id ?? task.initial_thread_id };
        task.resources.push({ kind: 'acp', id: created.thread_id });
        data = { kind: 'acp', session: created };
      }
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
      else if (path === '/api/projects/project/worktrees' && request.method() === 'POST') {
        worktreeRequests.push(request.postDataJSON());
      } else if (path === '/api/worktree-creations') data = [];
      else if (path.startsWith('/api/fs/tree')) {
        const root = decodeURIComponent(path.slice('/api/fs/tree'.length)); toolRequests.push(root);
        data = { path: root, entries: (root === '/notes' ? ['plan.md', 'ideas.md', 'note.md'] : names).map(name => ({ name, path: root + '/' + name, kind: 'file', readonly: false, size: 12, modified_ms: null, hidden: false, mode: 0, links: 1, uid: 0, gid: 0, is_symlink: false, link_target: null })) };
      }
      else if (path === '/api/fs/search') {
        const input = request.postDataJSON(); searchRequests.push(input);
        const name = input.root === '/notes' ? 'plan.md' : 'a.md';
        data = { files: [{ path: input.root + '/' + name, relative_path: name, matches: input.mode === 'content' ? [{ line: 1, column: 1, match_length: 4, text: 'plan', preview_column: 0, preview_length: 4 }] : [] }], total_matches: 1, truncated: false };
      }
      else if (path === '/api/fs/entries' && request.method() === 'PATCH') {
        const input = request.postDataJSON(); names = names.map(name => input.path.endsWith('/' + name) ? input.name : name);
        data = { path: input.path.slice(0, input.path.lastIndexOf('/') + 1) + input.name, name: input.name };
      }
      else if (path.startsWith('/api/fs/file') && request.method() === 'PUT') { writes.push(path); data = { path, version: '2' }; }
      else if (path === '/api/my-pull-requests') { toolRequests.push('pr:' + url.searchParams.get('repo')); data = { repository: url.searchParams.get('repo'), current_branch: 'main', current_user: detail.author, pull_requests: pullRequests }; }
      else if (/^\/api\/my-pull-requests\/\d+$/.test(path)) data = pullRequests.find(pr => pr.number === Number(path.split('/').at(-1)));
      else if (path.startsWith('/api/fs/text')) {
        if (path === '/api/fs/text/repo/slow.md') await slowRead;
        data = { path: path.slice('/api/fs/text'.length), content: '# ' + path, version: '1', language: 'markdown', mime: 'text/markdown' };
      }
      else if (path === '/api/fs/raw/repo/a.md') data = {};
      else if (path === '/api/projects/project/notes/temporary' && request.method() === 'POST') data = { name: 'note.md', path: '/notes/note.md' };
      else if (path === '/api/git/ignored') data = { repository: '/repo', ignored: [] };
      else if (path === '/api/git/status') { toolRequests.push(url.search); data = { repository: url.searchParams.get('repo') || '/repo', branch: 'main', ahead: 0, behind: 0, files: [{ path: 'changed.md', index_status: ' ', worktree_status: 'M', original_path: null }] }; }
      else if (path === '/api/git/diff') { toolRequests.push('diff:' + url.searchParams.get('repo')); data = { repository: url.searchParams.get('repo'), path: 'changed.md', staged: false, patch: '', original: 'before', modified: 'after', truncated: false }; }
      else if (path === '/api/git/log') data = { commits: [], repository: '/repo', upstream: null };
      else if (path === '/api/operations') data = { boot_id: 'fixture', revision: 0, operations: [] };
      return route.fulfill({ json: data });
    });
    const page = await context.newPage(); page.setDefaultTimeout(15000);
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/task-organization-preview.html`);
    await page.getByRole('button', { name: 'Organize by task', exact: true }).click();
    await page.locator('#aow-project-sidebar').getByRole('button', { name: 'New task', exact: true }).click();
    await page.getByLabel('Task description', { exact: true }).fill('Implement login');
    await page.getByRole('button', { name: 'Create task', exact: true }).click();
    const surface = page.locator('.project-aow-surface:visible');
    const center = surface.locator('.project-aow-center');
    const right = surface.locator('.project-aow-right');
    const toggleDetails = right.getByRole('button', { name: 'Toggle Details', exact: true });
    const maximize = () => right.getByRole('button', { name: 'Maximize Editor Area', exact: true }).click();
    const restore = () => right.getByRole('button', { name: 'Restore Editor Area', exact: true }).click();
    const details = right.locator('.agent-tools-details');
    await center.getByRole('tab').filter({ hasText: 'initial-0' }).waitFor();
    assert.equal(await toggleDetails.count(), 0);
    assert.equal(await page.getByRole('button', { name: 'Hide right sidebar', exact: true }).count(), 0);
    const openTool = async label => {
      await right.getByRole('button', { name: 'New tool', exact: true }).click();
      const menuItems = await page.locator('.project-aow-new-menu > button').allTextContents();
      assert.equal(menuItems.indexOf('Notes'), menuItems.indexOf('Files') + 1);
      await page.locator('.project-aow-new-menu').getByRole('button', { name: label, exact: true }).click();
    };
    const file = name => right.locator('.tree-list:visible').getByText(name, { exact: true });
    await center.getByRole('button', { name: 'New window', exact: true }).click();
    assert.equal(await page.locator('.project-aow-new-menu').getByRole('button', { name: 'Terminal', exact: true }).count(), 0);
    await page.keyboard.press('Escape');
    await openTool('Files');
    assert.equal(await right.getByRole('textbox', { name: 'Search notes', exact: true }).count(), 0);
    const projectSearch = right.getByRole('textbox', { name: 'Search project files', exact: true });
    await projectSearch.fill('project-query');
    await right.getByRole('tree', { name: 'Project Explorer search results' }).getByText('a.md', { exact: true }).waitFor();
    assert.deepEqual(searchRequests.map(request => request.root), ['/repo']);
    await projectSearch.press('Escape');
    await file('a.md').click();
    await right.getByRole('tab').filter({ hasText: 'a.md' }).waitFor();
    assert.equal(await center.getByRole('tab').count(), 1);
    assert.equal(await center.locator('.acp-tab-host:visible').count(), 1);
    const composer = center.getByRole('textbox', { name: 'ACP messages', exact: true });
    await composer.fill('Keep this unsent Agent draft');
    const originalRight = await right.boundingBox();
    const content = right.locator('.agent-tools-content');
    const originalContent = await content.boundingBox();
    assert.ok((await details.boundingBox()).x > originalContent.x);
    await toggleDetails.click();
    assert.equal(await details.isVisible(), false);
    assert.ok((await content.boundingBox()).width > originalContent.width + 100);
    assert.equal((await right.boundingBox()).x, originalRight.x);
    await maximize();
    assert.equal(await center.isVisible(), false);
    assert.equal(await surface.locator('.project-aow-right-resizer').isVisible(), false);
    assert.equal(await page.locator('#aow-project-sidebar').isVisible(), true);
    assert.ok(Math.abs((await right.boundingBox()).width - (await surface.boundingBox()).width) < 2);
    assert.equal(await toggleDetails.getAttribute('aria-pressed'), 'false');
    await toggleDetails.click();
    assert.equal(await details.isVisible(), true);
    await page.screenshot({ path: '/tmp/aow-editor-maximized.png' });
    await page.getByRole('button', { name: 'Hide left sidebar', exact: true }).click();
    await page.setViewportSize({ width: 900, height: 800 });
    await right.getByRole('button', { name: 'Show left sidebar', exact: true }).click();
    await restore();
    await page.setViewportSize({ width: 1440, height: 1000 });
    assert.ok(Math.abs((await right.boundingBox()).width - originalRight.width) < 2);
    assert.equal(await composer.innerText(), 'Keep this unsent Agent draft');
    await right.getByRole('tab', { selected: true }).filter({ hasText: 'a.md' }).waitFor();
    await file('b.md').click();
    await right.getByRole('tab').filter({ hasText: 'b.md' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'a.md' }).count(), 0);
    await right.getByRole('tab').filter({ hasText: 'b.md' }).dblclick();
    await file('c.md').click();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'b.md' }).count(), 1);
    await right.locator('.monaco-editor').first().waitFor();
    await page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      const model = monaco.editor.getModels().find(model => model.uri.path.endsWith('/c.md'));
      if (!model) throw new Error('Expected c.md editor');
      model.setValue('# Edited file');
    });
    await page.waitForFunction(() => !document.querySelector('[data-workspace-tab-id="document:/repo/c.md"]')?.classList.contains('preview'));
    await page.waitForResponse(response => response.url().endsWith('/api/fs/file/repo/c.md') && response.request().method() === 'PUT');
    assert.ok(writes.includes('/api/fs/file/repo/c.md'));
    await toggleDetails.click();
    await maximize();
    await restore();
    assert.equal(await page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      return monaco.editor.getModels().find(model => model.uri.path.endsWith('/c.md'))?.getValue();
    }), '# Edited file');
    const kept = right.getByRole('tab').filter({ hasText: 'b.md' });
    await kept.click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Rename', exact: true }).click();
    await right.getByRole('textbox', { name: 'File name', exact: true }).fill('renamed.md');
    await page.keyboard.press('Enter');
    await right.getByRole('tab').filter({ hasText: 'renamed.md' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'b.md' }).count(), 0);
    assert.equal(await toggleDetails.getAttribute('aria-pressed'), 'false');
    await openTool('Source Control');
    assert.equal(await details.isVisible(), false);
    await toggleDetails.click();
    await right.locator('.source-control').getByText('changed.md', { exact: true }).click();
    await right.getByRole('tab').filter({ hasText: 'changed.md (Diff)' }).waitFor();
    assert.ok(toolRequests.includes('diff:/repo'));
    await toggleDetails.click();
    assert.equal(await details.isVisible(), false);
    assert.equal(await content.isVisible(), true);
    await toggleDetails.click();
    await openTool('Pull Requests');
    await right.locator('.my-pr-list').getByText(pullRequests[0].title, { exact: false }).click();
    await right.getByRole('tab').filter({ hasText: 'PR #42' }).waitFor();
    await right.getByRole('tab').filter({ hasText: 'PR #42' }).dblclick();
    await right.locator('.my-pr-list').getByText(pullRequests[1].title, { exact: false }).click();
    await right.getByRole('tab').filter({ hasText: 'PR #43' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'PR #42' }).count(), 1);
    await toggleDetails.click();
    assert.equal(await details.isVisible(), false);
    assert.equal(await content.isVisible(), true);
    await openTool('Notes');
    await right.getByRole('tab', { selected: true }).filter({ hasText: 'Notes' }).waitFor();
    await openTool('New Markdown');
    await right.getByRole('tab').filter({ hasText: 'note.md' }).waitFor();
    assert.equal(await toggleDetails.getAttribute('aria-pressed'), 'false');
    await toggleDetails.click();
    assert.equal(await right.getByRole('textbox', { name: 'Search project files', exact: true }).count(), 0);
    const notesSearch = right.getByRole('textbox', { name: 'Search notes', exact: true });
    await notesSearch.fill('notes-query');
    const notesResults = right.getByRole('tree', { name: 'Notes Explorer search results' });
    await notesResults.getByText('plan.md', { exact: true }).waitFor();
    await right.getByRole('button', { name: 'Content', exact: true }).click();
    await notesResults.locator('mark').click();
    await right.getByRole('tab', { selected: true }).filter({ hasText: 'plan.md' }).dblclick();
    assert.deepEqual(searchRequests.filter(request => request.query === 'notes-query').map(request => [request.root, request.mode]), [['/notes', 'name'], ['/notes', 'content']]);
    await notesSearch.press('Escape');
    await file('ideas.md').click();
    await right.getByRole('tab').filter({ hasText: 'ideas.md' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'plan.md' }).count(), 1);
    assert.ok((await details.boundingBox()).x > (await content.boundingBox()).x);
    await page.screenshot({ path: '/tmp/aow-notes-tools.png' });
    await toggleDetails.click();
    assert.equal(await center.getByRole('tab').count(), 1);
    await center.getByRole('button', { name: 'New window', exact: true }).click();
    await page.locator('.project-aow-new-menu').getByRole('button', { name: 'Reviewer ACP', exact: true }).click();
    await center.getByRole('tab').filter({ hasText: 'helper-' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'renamed.md' }).count(), 0);
    await openTool('Files');
    assert.equal(await toggleDetails.getAttribute('aria-pressed'), 'true');
    await file('a.md').click();
    await page.waitForFunction(() => document.querySelector('.agent-tools-content .project-aow-document-host:not([hidden])')?.textContent.includes('/repo-other'));
    assert.ok(toolRequests.includes('/repo-other'));
    await openTool('Source Control');
    await right.locator('.source-control').getByText('changed.md', { exact: true }).click();
    await right.getByRole('tab').filter({ hasText: 'changed.md (Diff)' }).waitFor();
    assert.ok(toolRequests.includes('diff:/repo-other'));
    await openTool('Pull Requests');
    await right.locator('.my-pr-list').getByText(pullRequests[0].title, { exact: false }).click();
    await right.getByRole('tab').filter({ hasText: 'PR #42' }).waitFor();
    assert.ok(toolRequests.includes('pr:/repo-other'));
    await openTool('Terminal');
    await right.getByRole('tab').filter({ hasText: 'Shell' }).waitFor();
    assert.equal(terminals.at(-1).panes[0].cwd, '/repo-other');
    assert.equal(await toggleDetails.count(), 0);
    const terminalCount = terminals.length;
    await maximize();
    await restore();
    assert.equal(terminals.length, terminalCount);
    assert.deepEqual(deleted, []);
    assert.equal(await center.getByRole('tab').count(), 2);
    await center.getByRole('tab').filter({ hasText: 'initial-0' }).click();
    await right.getByRole('tab').filter({ hasText: 'renamed.md' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'Shell' }).count(), 0);
    await right.getByRole('tab').filter({ hasText: 'renamed.md' }).click();
    assert.equal(await toggleDetails.getAttribute('aria-pressed'), 'false');
    await page.screenshot({ path: '/tmp/aow-agent-tools-files.png' });
    await right.getByRole('tab').filter({ hasText: 'Properties' }).click();
    await right.getByRole('region', { name: 'Properties', exact: true }).waitFor();
    assert.equal(await toggleDetails.count(), 0);
    assert.equal(await right.getByRole('region', { name: 'Conversation', exact: true }).count(), 1);
    await maximize();
    await right.locator('.conversation-instance').filter({ hasText: 'initial-0' }).locator('.aow-list-row-open').click();
    assert.equal(await center.isVisible(), true);
    await right.getByRole('button', { name: 'Maximize Editor Area', exact: true }).waitFor();
    await page.screenshot({ path: '/tmp/aow-agent-tools-layout.png' });
    await page.reload();
    await center.getByRole('tab').filter({ hasText: 'initial-0' }).waitFor();
    await right.getByRole('tab').filter({ hasText: 'renamed.md' }).waitFor();
    await right.getByRole('tab').filter({ hasText: 'c.md' }).waitFor();
    await right.getByRole('tab').filter({ hasText: 'plan.md' }).click();
    await toggleDetails.click();
    await right.getByRole('textbox', { name: 'Search notes', exact: true }).waitFor();
    assert.equal(await right.getByRole('textbox', { name: 'Search project files', exact: true }).count(), 0);
    await toggleDetails.click();
    await right.getByRole('tab').filter({ hasText: 'renamed.md' }).click();
    assert.equal(await toggleDetails.getAttribute('aria-pressed'), 'false');
    await toggleDetails.click();
    slowRead = new Promise(resolve => { releaseSlow = resolve; });
    await file('slow.md').click();
    await center.getByRole('tab').filter({ hasText: 'helper-' }).click();
    await right.getByRole('tab').filter({ hasText: 'Shell' }).waitFor();
    const response = page.waitForResponse(response => response.url().endsWith('/api/fs/text/repo/slow.md'));
    releaseSlow(); await response;
    assert.equal(await right.getByRole('tab', { selected: true }).innerText(), 'Shell');
    await page.reload();
    await center.getByRole('tab', { selected: true }).filter({ hasText: 'helper-' }).waitFor();
    await right.getByRole('tab', { selected: true }).filter({ hasText: 'Shell' }).waitFor();
    assert.equal(await right.getByRole('tab').filter({ hasText: 'renamed.md' }).count(), 0);
    assert.deepEqual(errors, []);
  });
} finally {
  await browser?.close();
  await server.close();
}
