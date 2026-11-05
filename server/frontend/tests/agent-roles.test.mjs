import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  await test('agent wizard selects ACP and Agent CLI instances in one step and retains workspace/task integration', async t => {
    const context = await browser.newContext({ viewport: { width: 1280, height: 1000 } });
    t.after(() => context.close());
    await installLiveEvents(context);
    await context.addInitScript(() => {
      localStorage.setItem('aow.language', 'en-US');
      // LAN HTTP deployments do not provide crypto.randomUUID.
      Object.defineProperty(crypto, 'randomUUID', { value: undefined });
    });
    const roles = [], launches = [], tasks = [], runtimeRequests = [], probes = [], browsed = [];
    const acp = { id: 'codex-acp', name: 'Codex ACP', supported: true, installed: true, configured: true };
    const settings = { content: '{"agent_servers":{}}', revision: '1' };
    const identities = [{ layout: 'codex', source: '/local' }, { layout: 'claude', source: '/local' }, { layout: 'traecli', source: '/local' }];
    const unknown = { id: 'my-agent', name: 'Custom Agent', supported: true, installed: true, configured: true };
    let finishProbe;
    const cli = { id: 'codex-acp', agent_type: 'codex', display_name: 'Local Codex', command: 'codex', executable: '/tools/codex', args: ['--profile', 'work'], env: {}, available: true, source: 'configured' };
    const traecli = { ...cli, id: 'local-traecli', agent_type: 'traecli', display_name: 'Local TraeCLI', command: 'traecli', executable: '/tools/traecli', args: [] };
    const claude = { ...cli, id: 'local-claude', agent_type: 'claude', display_name: 'Local Claude', command: 'claude', executable: '/tools/claude', args: [] };
    let cliList = [cli, claude, traecli, { ...cli, id: 'other', agent_type: 'pi', display_name: 'Pi' }, { ...cli, id: 'detected-only', source: 'detected' }, { ...cli, id: 'unavailable-cli', available: false }];
    let acpList = [acp, unknown, { ...acp, id: 'codex-uninstalled', installed: false }, { ...acp, id: 'codex-unsupported', supported: false }, { ...acp, id: 'codex-unconfigured', configured: false }];
    await context.route('**/api/**', async route => {
      const request = route.request(), path = new URL(request.url()).pathname;
      if (path.startsWith('/api/acp/') || path === '/api/agents') runtimeRequests.push({ path, method: request.method() });
      let data = [];
      if (path.startsWith('/api/fs/')) browsed.push(path);
      else if (path === '/api/settings') data = { notes_base: '/notes', execution_path: ['/tools'], editor: {} };
      else if (path === '/api/agents') data = cliList;
      else if (path === '/api/agent-roles/identity-sources') data = identities;
      else if (path === '/api/agent-roles/test-and-create') {
        const body = request.postDataJSON(); probes.push(body);
        if (probes.length === 1) {
          await new Promise(resolve => { finishProbe = resolve; });
          return route.fulfill({ status: 400, json: { message: 'Login expired; refresh the local identity and retry' } });
        }
        const config = { ...body }; delete config.request_id;
        data = { ...config, home_layout: config.runtime.mode === 'cli' ? cliList.find(item => item.id === config.runtime.agent_id).agent_type : 'codex', id: `role-${roles.length}` }; roles.push(data);
      } else if (path === '/api/agent-roles') {
        assert.equal(request.method(), 'GET', 'creation must use the tested endpoint'); data = roles;
      } else if (path.match(/^\/api\/agent-roles\/[^/]+\/sessions$/)) {
        launches.push({ path, ...request.postDataJSON() }); data = { kind: 'acp', session: { thread_id: 'session', agent_id: acp.id } };
      } else if (path.match(/^\/api\/agent-roles\/[^/]+$/) && request.method() === 'PUT') {
        const role = roles.find(item => path.endsWith(`/${item.id}`)); Object.assign(role, request.postDataJSON()); data = role;
      } else if (path === '/api/acp/settings') data = settings;
      else if (path === '/api/acp/agents') data = acpList;
      else if (path === '/api/tasks' && request.method() === 'POST') {
        data = { ...request.postDataJSON(), status: 'in_progress', created_at: null, updated_at: null, completed_at: null, id: 'task-role', resources: [] }; tasks.push(data);
      } else if (path === '/api/projects') data = [{ id: 'p', name: 'Project', registered_path: '/repo', worktrees: [{ id: 'w', path: '/repo', branch: 'main', is_main: true }] }];
      return route.fulfill({ json: data });
    });
    const page = await context.newPage(); page.setDefaultTimeout(15000);
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/tests/agent-roles-preview.html`);
    await page.getByRole('button', { name: /^Agents /, exact: false }).click();
    const editor = page.getByRole('region', { name: 'Add Agent', exact: true });
    const next = () => editor.getByRole('button', { name: 'Next', exact: true }).click();
    const create = () => editor.getByRole('button', { name: 'Test connection and create' }).click();
    const noDialogs = async () => assert.equal(await page.getByRole('dialog').count(), 0);
    const start = async name => {
      await page.getByRole('button', { name: 'Add Agent', exact: true }).click();
      await editor.getByRole('textbox', { name: 'Agent name', exact: true }).fill(name);
    };
    await start('Reviewer');
    await editor.getByRole('textbox', { name: 'Description', exact: true }).fill('Review changes carefully');
    assert.equal(await editor.getByRole('combobox').count(), 0);
    await next(); await noDialogs();
    assert.equal(await editor.getByRole('tab').count(), 0);
    assert.equal(await editor.getByRole('button', { name: 'Install', exact: true }).count(), 0);
    await editor.getByLabel('Runtime', { exact: true }).selectOption('acp:codex-acp');
    await noDialogs();
    await editor.getByRole('button', { name: 'Back', exact: true }).click();
    assert.equal(await editor.getByRole('textbox', { name: 'Description', exact: true }).inputValue(), 'Review changes carefully');
    await next(); await next();
    assert.equal(await editor.getByLabel('Source Home', { exact: true }).inputValue(), '/local');
    assert.equal(await editor.getByRole('textbox').count(), 1, 'known agents only expose the source Home');
    assert.equal(await editor.getByRole('combobox').count(), 0);
    assert.equal(await editor.getByRole('button', { name: 'Add identity file' }).count(), 0);
    await editor.getByLabel('Source Home', { exact: true }).fill('/custom/codex-home');
    await create();
    await editor.getByText('Testing the runtime in the isolated Home…', { exact: true }).waitFor();
    assert.equal(await editor.getByRole('button', { name: 'Back', exact: true }).isDisabled(), true);
    assert.equal(roles.length, 0);
    finishProbe();
    await editor.getByRole('alert').waitFor();
    assert.match(await editor.getByRole('alert').textContent(), /Login expired/);
    assert.equal(roles.length, 0);
    await create(); await editor.waitFor({ state: 'hidden' });
    assert.equal(probes[0].request_id, probes[1].request_id);
    assert.equal(probes[0].home_layout, undefined);
    assert.equal(roles[0].description, 'Review changes carefully');
    assert.equal(roles[0].runtime.agent_id, 'codex-acp');
    assert.equal(roles[0].identity.source, '/custom/codex-home');
    assert.deepEqual(roles[0].identity, { kind: 'local_home', source: '/custom/codex-home' });
    assert.deepEqual(roles[0].skills, []);

    // Both runtime types share one selector; unavailable and unsupported entries stay visible.
    await start('Supported runtime draft'); await next();
    assert.equal(await editor.getByRole('combobox').count(), 1);
    assert.deepEqual(await editor.getByLabel('Runtime', { exact: true }).locator('optgroup').evaluateAll(items => items.map(item => item.label)), ['ACP', 'Agent CLI']);
    assert.deepEqual(await editor.getByLabel('Runtime', { exact: true }).locator('optgroup[label=ACP] option').evaluateAll(items => items.map(item => item.value)), acpList.map(item => `acp:${item.id}`));
    assert.deepEqual(await editor.getByLabel('Runtime', { exact: true }).locator('optgroup[label="Agent CLI"] option').evaluateAll(items => items.map(item => item.value)), cliList.map(item => `cli:${item.id}`));
    assert.equal(await editor.locator('option[value="acp:codex-uninstalled"]').evaluate(option => option.disabled), true);
    assert.equal(await editor.locator('option[value="acp:codex-unsupported"]').evaluate(option => option.disabled), true);
    assert.equal(await editor.locator('option[value="acp:my-agent"]').evaluate(option => option.disabled), true);
    assert.equal(await editor.locator('option[value="acp:codex-unconfigured"]').evaluate(option => option.disabled), true);
    await noDialogs();
    await editor.getByRole('button', { name: 'Cancel', exact: true }).click();
    assert.equal(roles.length, 1);

    await start('CLI helper'); await next();
    assert.equal(await editor.getByLabel('CLI type', { exact: true }).count(), 0);
    assert.equal(await editor.getByLabel('Executable', { exact: true }).count(), 0);
    assert.equal(await editor.getByLabel('Runtime', { exact: true }).locator('option[value="cli:detected-only"]').evaluate(option => option.disabled), false);
    assert.equal(await editor.getByLabel('Runtime', { exact: true }).locator('option[value="cli:unavailable-cli"]').evaluate(option => option.disabled), true);
    assert.equal(await editor.getByLabel('Runtime', { exact: true }).locator('option[value="cli:other"]').evaluate(option => option.disabled), true);
    await editor.getByLabel('Runtime', { exact: true }).selectOption('cli:codex-acp');
    await next(); await create(); await editor.waitFor({ state: 'hidden' });
    assert.deepEqual(roles[1].runtime, { mode: 'cli', agent_id: 'codex-acp' }, 'ACP and CLI may have the same ID without colliding');
    assert.deepEqual(roles[1].identity, { kind: 'local_home', source: '/local' });
    assert.deepEqual(browsed, []);
    assert.ok(runtimeRequests.every(request => request.method === 'GET'));
    assert.ok(runtimeRequests.every(request => !/registry|installation/.test(request.path)));

    await page.getByRole('article').filter({ hasText: 'Reviewer' }).getByRole('button', { name: 'Edit', exact: true }).click();
    const editing = page.getByRole('region', { name: 'Edit Agent', exact: true });
    assert.equal(await editing.getByLabel('Runtime', { exact: true }).inputValue(), 'acp:codex-acp');
    await editing.getByLabel('Instructions', { exact: true }).fill('Use the review skill');
    await editing.getByLabel('Skills', { exact: true }).fill('/skills/review');
    await editing.getByRole('button', { name: 'Save agent' }).click();
    await editing.waitFor({ state: 'hidden' });
    assert.equal(roles[0].instruction, 'Use the review skill');
    assert.deepEqual(roles[0].skills, ['/skills/review']);

    await page.getByRole('button', { name: 'Workspace creation', exact: true }).click();
    await page.getByRole('button', { name: 'Reviewer ACP', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('output')?.textContent?.includes('session'));
    assert.equal(launches[0].path, '/api/agent-roles/role-0/sessions');
    await page.getByRole('button', { name: 'Create task', exact: true }).click();
    await page.getByRole('textbox', { name: 'Task description' }).fill('Investigate the change');
    await page.getByRole('button', { name: 'Initial agent', exact: true }).click();
    assert.equal(await page.getByRole('option', { name: /CLI helper/ }).count(), 0, 'task execution currently supports ACP agents');
    await page.getByRole('option', { name: /Reviewer/ }).click();
    await page.getByRole('button', { name: 'Create task', exact: true }).last().click();
    await page.waitForFunction(() => document.querySelector('output')?.textContent?.includes('task-role'));
    assert.equal(tasks[0].agent_role_id, 'role-0'); assert.equal(tasks[0].agent_id, '');
    await page.getByRole('button', { name: 'Settings', exact: true }).click();
    await page.getByRole('button', { name: /^Agents /, exact: false }).click();
    await start('New role'); await next();
    await editor.getByLabel('Runtime', { exact: true }).selectOption('acp:codex-acp');
    await page.screenshot({ path: '/tmp/aow-agent-roles-desktop.png', fullPage: true });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: '/tmp/aow-agent-roles-mobile.png', fullPage: true });
    assert.equal(await editor.evaluate(element => element.scrollWidth <= element.clientWidth + 1), true);
    await noDialogs();
    await editor.getByLabel('Runtime', { exact: true }).selectOption('cli:local-claude');
    await next();
    assert.equal(await editor.getByLabel('Source Home', { exact: true }).inputValue(), '/local');
    assert.equal(await editor.getByRole('textbox').count(), 1);
    assert.equal(await editor.getByRole('button', { name: 'Add identity file' }).count(), 0);
    await editor.getByLabel('Source Home', { exact: true }).fill('/another/home');
    await page.screenshot({ path: '/tmp/aow-role-import-mobile.png', fullPage: true });
    assert.equal(await editor.evaluate(element => element.scrollWidth <= element.clientWidth + 1), true);
    assert.equal(await editor.getByRole('button', { name: 'Test connection and create' }).isEnabled(), true);
    await create(); await editor.waitFor({ state: 'hidden' });
    assert.deepEqual(roles[2].identity, { kind: 'local_home', source: '/another/home' });
    assert.deepEqual(roles[2].runtime, { mode: 'cli', agent_id: 'local-claude' });
    await start('TraeCLI helper'); await next();
    await editor.getByLabel('Runtime', { exact: true }).selectOption('cli:local-traecli');
    await next();
    assert.equal(await editor.getByLabel('Source Home', { exact: true }).inputValue(), '/local');
    assert.equal(await editor.getByRole('textbox').count(), 1);
    assert.equal(await editor.getByRole('button', { name: 'Add identity file' }).count(), 0);
    await editor.getByLabel('Source Home', { exact: true }).fill('/trae-source');
    await create(); await editor.waitFor({ state: 'hidden' });
    assert.deepEqual(roles[3].identity, { kind: 'local_home', source: '/trae-source' });
    assert.deepEqual(roles[3].runtime, { mode: 'cli', agent_id: 'local-traecli' });
    assert.deepEqual(browsed, []);
    // A missing runtime remains visible on existing roles and disables saving.
    cliList = [];
    await page.getByRole('article').filter({ has: page.getByText('CLI helper', { exact: true }) }).getByRole('button', { name: 'Edit', exact: true }).click();
    await editing.getByRole('option', { name: /Agent CLI · codex-acp · Unavailable/ }).waitFor({ state: 'attached' });
    assert.equal(await editing.getByLabel('Runtime', { exact: true }).inputValue(), 'cli:codex-acp');
    assert.equal(await editing.getByRole('button', { name: 'Save agent' }).isDisabled(), true);
    cliList = [cli];
    await editing.getByRole('button', { name: 'Refresh runtime list' }).click();
    await editing.getByRole('button', { name: 'Save agent' }).click();
    await editing.waitFor({ state: 'hidden' });

    acpList = [];
    await start('Runtime configured elsewhere'); await next();
    await editor.getByRole('button', { name: 'Refresh runtime list' }).waitFor();
    assert.equal(await editor.getByLabel('Runtime', { exact: true }).locator('optgroup[label=ACP] option').count(), 0);
    assert.equal(await editor.getByRole('button', { name: 'Next', exact: true }).isDisabled(), true);
    acpList = [acp];
    await editor.getByRole('button', { name: 'Refresh runtime list' }).click();
    await editor.getByLabel('Runtime', { exact: true }).selectOption('acp:codex-acp');
    await next();
    await editor.getByRole('button', { name: 'Cancel', exact: true }).click();
    assert.ok(runtimeRequests.every(request => request.method === 'GET'));
    assert.ok(runtimeRequests.every(request => !/registry|installation/.test(request.path)));
    assert.deepEqual(errors, []);
  });
} finally { await browser?.close(); await server.close(); }
