import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
const item = (kind, id, parent = null, cwd = '/repo', deleted = false) => ({ kind, id, parent, cwd, deleted, title: id, agent_id: 'codex', url: null, ...(kind === 'terminal' && { tab_id: `tab-${id}` }) });
const ref = (kind, id) => ({ kind, id });
const flatten = nodes => nodes.flatMap(node => [node.instance.id, ...flatten(node.children)]);
let browser;
try {
  await server.listen();
  const { conversationTree, searchConversations } = await server.ssrLoadModule('/src/features/conversations/tree.ts');
  const { taskResourceOwners } = await server.ssrLoadModule('/src/features/tasks/membership.ts');
  const graph = [item('acp', 'root'), item('terminal', 'agent', ref('acp', 'root')), item('acp', 'child', ref('terminal', 'agent'), '/branch'),
    { ...item('terminal', 'shell', ref('acp', 'child'), '/branch'), agent_id: null }, item('acp', 'grandchild', ref('terminal', 'shell'), '/branch'),
    item('acp', 'other'), item('acp', 'foreign', null, '/foreign')];
  await test('worktree tree includes mixed descendants once, bridges shells and excludes unrelated worktrees', () => {
    const tree = conversationTree(graph, '/repo');
    assert.deepEqual(flatten(tree), ['root', 'agent', 'child', 'grandchild', 'other']);
    assert.equal(tree[0].children[0].children[0].instance.id, 'child');
    assert.deepEqual(flatten(searchConversations(tree, 'grandchild')), ['root', 'agent', 'child', 'grandchild']);
  });
  await test('task scope follows ownership and excludes dependencies and another tasks explicitly assigned descendants', () => {
    const owners = taskResourceOwners([
      { id: 'task', resources: [ref('acp', 'root')], dependencies: ['other'] },
      { id: 'other', resources: [ref('acp', 'other'), ref('acp', 'grandchild')] },
    ], graph);
    assert.deepEqual(flatten(conversationTree(graph, '/repo', (kind, id) => owners.get(`${kind}:${id}`) === 'task')), ['root', 'agent', 'child']);
  });
  await test('deleted ancestors remain only for surviving descendants and malformed ancestry never loses live instances', () => {
    const tree = conversationTree(graph.map(info => ['root', 'agent', 'other'].includes(info.id) ? { ...info, deleted: true } : info), '/repo');
    assert.deepEqual(flatten(tree), ['root', 'agent', 'child', 'grandchild']);
    const cycle = conversationTree([item('acp', 'a', ref('acp', 'b')), item('acp', 'b', ref('acp', 'a')),
      item('acp', 'missing', ref('acp', 'absent')), item('acp', 'self', ref('acp', 'self'))], '/repo');
    assert.deepEqual(flatten(cycle).sort(), ['a', 'b', 'missing', 'self']);
  });
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('Conversation switches scope, opens agent panes, retains deleted parents and searches through ancestry', async t => {
    const context = await browser.newContext();
    t.after(() => context.close());
    await context.addInitScript(() => localStorage.setItem('aow.language', 'en-US'));
    const instances = structuredClone(graph);
    const tabs = instances.filter(info => info.kind === 'terminal').map(info => ({ id: info.tab_id, name: info.title, workspace_root: info.cwd,
      layout: { type: 'pane', pane_id: info.id }, panes: [{ id: info.id, name: info.title, cwd: info.cwd, agent_id: info.agent_id, parent: info.parent, shell: 'sh', status: 'running' }] }));
    await installLiveEvents(context, undefined, () => ({ tabs, metadata: { agents: {}, titles: {}, processes: {}, activity: {} }, error: null, agent_error: null }));
    const sessionPaths = new Set();
    await context.route('**/api/**', route => {
      const url = new URL(route.request().url());
      if (url.pathname === '/api/instances') return route.fulfill({ json: { items: instances } });
      if (url.pathname === '/api/acp/sessions') {
        const cwd = url.searchParams.get('cwd'); sessionPaths.add(cwd);
        return route.fulfill({ json: instances.filter(info => info.kind === 'acp' && !info.deleted && info.cwd === cwd).map(info => ({
          ...info, thread_id: info.id, created_at: '2026-01-01', updated_at: '2026-01-01', status: 'idle', source: 'user', session_id: info.id,
        })) });
      }
      if (url.pathname === '/api/notifications') return route.fulfill({ json: { items: [], revision: 0, unread_count: 0 } });
      return route.fulfill({ json: [] });
    });
    const page = await context.newPage();
    page.setDefaultTimeout(10000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/conversation-preview.html`);
    const panel = page.locator('.conversation-panel');
    await panel.locator('[data-instance="acp:grandchild"]').waitFor();
    assert.equal(await panel.locator('[data-instance]').count(), 5);
    assert.equal(await panel.locator('[data-instance="terminal:shell"]').count(), 0);
    assert.equal(await panel.locator('[data-instance="acp:root"] [data-instance="terminal:agent"] [data-instance="acp:child"]').count(), 1);
    await panel.getByRole('button', { name: 'agent', exact: true }).click();
    assert.equal(await page.getByLabel('Opened instance').textContent(), 'terminal:tab-agent');
    await page.getByRole('button', { name: 'worktree', exact: true }).click();
    await panel.locator('[data-instance="acp:other"]').waitFor({ state: 'detached' });
    await panel.locator('[data-instance="acp:grandchild"]').waitFor();
    assert.equal(await panel.locator('[data-instance]').count(), 4);
    assert.equal(await panel.locator('.instance-worktree-heading').count(), 0);
    instances.find(info => info.id === 'agent').deleted = true;
    tabs.splice(tabs.findIndex(tab => tab.id === 'tab-agent'), 1);
    await panel.getByRole('button', { name: 'Refresh conversations', exact: true }).click();
    await panel.locator('[data-instance="terminal:agent"][data-deleted]').waitFor();
    assert.equal(await panel.locator('[data-instance="terminal:agent"] > .conversation-row .aow-list-row-open').isDisabled(), true);
    await page.screenshot({ path: '/tmp/aow-conversation-panel.png' });
    await panel.getByRole('textbox', { name: 'Search conversations', exact: true }).fill('grandchild');
    assert.equal(await panel.locator('[data-instance]').count(), 4);
    await panel.getByRole('textbox').fill('unrelated');
    await panel.getByText('No conversations', { exact: true }).waitFor();
    assert.deepEqual([...sessionPaths].sort(), ['/branch', '/repo']);
    assert.deepEqual(errors, []);
  });
} finally { await browser?.close(); await server.close(); }
