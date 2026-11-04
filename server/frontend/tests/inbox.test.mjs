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
  for (const mobile of [false, true]) {
    await test('durable Inbox filters, reads, archives and retries returns on ' + (mobile ? 'mobile' : 'desktop'), async t => {
      const context = await browser.newContext({ viewport: mobile ? { width: 390, height: 844 } : { width: 1400, height: 960 } });
      t.after(() => context.close());
      await context.addInitScript(() => localStorage.setItem('aow.language', 'en-US'));
      await installLiveEvents(context);
      await acpControlFixture().install(context);
      const project = { id: 'project', name: 'Task project', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes',
        worktrees: [{ id: 'wt', project_id: 'project', path: '/repo', branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false }] };
      const items = [{ id: 'result-one', read: false, archived: false, created_at: Date.now(), delivery: { state: 'delivered', error: null },
        request_id: 'report-one', task_id: 'child', project_id: 'project', thread_id: 'child-thread', kind: 'result', message: '104151 lines of Rust',
        target: { task_id: 'router', thread_id: 'router-thread' }, source: { task_id: 'router', thread_id: 'router-thread', request: '@Developer count Rust code', turn_id: 'turn-one', message_id: 'message-one' } }];
      let retries = 0;
      await context.route('**/api/**', async route => {
        const request = route.request(), url = new URL(request.url()), path = url.pathname;
        let data = [];
        if (path === '/api/auth/status') data = { configured: true, authenticated: true };
        else if (path === '/api/inbox') {
          const visible = items.filter(item => item.archived === (url.searchParams.get('archived') === 'true')
            && (url.searchParams.get('unread') !== 'true' || !item.read)
            && (!url.searchParams.get('project_id') || item.project_id === url.searchParams.get('project_id')));
          data = { items: visible, unread: items.filter(item => !item.read && !item.archived).length, has_more: false };
        } else if (path.startsWith('/api/inbox/')) {
          const item = items.find(item => item.id === path.split('/')[3]);
          if (path.endsWith('/retry')) { retries++; item.delivery = { state: 'pending', error: null }; }
          else Object.assign(item, request.postDataJSON());
          return route.fulfill({ status: 204 });
        } else if (path === '/api/aow/projects') data = [project];
        else if (path === '/api/aow/settings') data = { notes_base: '/notes', execution_path: [], editor: {} };
        else if (path.includes('/pinned-')) data = { paths: [], revision: 0 };
        else if (path === '/api/instances') data = { items: [] };
        else if (path === '/api/terminals/agents') data = { agents: {}, titles: {}, processes: {} };
        else if (path === '/api/operations') data = { boot_id: 'fixture', revision: 0, operations: [] };
        else if (path.startsWith('/api/fs/tree')) data = { path: '/repo', entries: [] };
        else if (path === '/api/git/status') data = { repository: '/repo', branch: 'main', files: [] };
        else if (path === '/api/git/log') data = { commits: [], repository: '/repo', upstream: null };
        return route.fulfill({ json: data });
      });
      const page = await context.newPage();
      page.setDefaultTimeout(20000);
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.goto(server.resolvedUrls.local[0] + (mobile ? 'm' : 'aow/'));
      await page.getByRole('button', { name: 'Inbox', exact: true }).click();
      const inbox = page.getByRole('region', { name: 'Inbox', exact: true });
      await inbox.getByText('104151 lines of Rust', { exact: true }).first().waitFor();
      await inbox.getByText('View details', { exact: true }).click();
      await inbox.getByText('@Developer count Rust code', { exact: true }).waitFor();
      await inbox.getByRole('button', { name: 'Mark unread', exact: true }).waitFor();
      assert.equal(items[0].read, true);
      assert.equal(items[0].delivery.state, 'delivered', 'reading does not acknowledge or retry a return');
      await inbox.getByRole('button', { name: 'Archive', exact: true }).click();
      await inbox.getByText('No conversation notifications yet.', { exact: true }).waitFor();
      await inbox.getByRole('combobox', { name: 'Filter', exact: true }).selectOption('archived');
      await inbox.getByRole('button', { name: 'Restore', exact: true }).click();
      await inbox.getByRole('combobox', { name: 'Filter', exact: true }).selectOption('all');
      items.push({ ...items[0], id: 'result-two', read: false, archived: false,
        kind: 'blocked', message: 'Review login: Provider unavailable',
        delivery: { state: 'blocked', error: 'Source cannot be resumed' } });
      await page.evaluate(() => window.emitLiveEvent('workspace', { boot_id: 'watch-fixture', revision: 1, projects: 0, terminals: 0, inbox: 1, repositories: {} }));
      await inbox.getByText('Source cannot be resumed', { exact: false }).waitFor();
      assert.equal(await inbox.locator('.inbox-row').count(), 2);
      await inbox.getByRole('button', { name: 'Retry return', exact: true }).click();
      await inbox.getByText('Delivery queued', { exact: false }).waitFor();
      assert.equal(retries, 1);
      assert.equal(items[1].message, 'Review login: Provider unavailable', 'retrying delivery preserves the explicit report');
      await inbox.getByRole('combobox', { name: 'Filter', exact: true }).selectOption('unread');
      await inbox.locator('.inbox-row').filter({ hasText: 'Review login' }).waitFor();
      assert.equal(await inbox.locator('.inbox-row').count(), 1);
      assert.deepEqual(errors, []);
      await page.screenshot({ path: mobile ? '/tmp/aow-inbox-mobile.png' : '/tmp/aow-inbox-desktop.png' });
    });
  }
} finally { await browser?.close(); await server.close(); }
