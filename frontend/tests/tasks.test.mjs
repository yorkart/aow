import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  async function fixture(t, { clockTime } = {}) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    page.setDefaultTimeout(15000);
    if (clockTime) await page.clock.install({ time: clockTime });
    t.after(() => page.close());
    await installLiveEvents(page, { boot_id: 'tasks-fixture', revision: 0, projects: 0, terminals: 0, tasks: 0, repositories: {} });
    await page.addInitScript(() => {
      // LAN HTTP pages cannot rely on the secure-context-only randomUUID API.
      Object.defineProperty(crypto, 'randomUUID', { value: undefined });
      window.emitTaskBoardChange = revision => window.emitLiveEvent('workspace', { boot_id: 'tasks-fixture', revision, projects: 0, terminals: 0, tasks: revision, repositories: {} });
    });
    const errors = []; page.on('pageerror' , e => errors.push(e.message));
    const now = (clockTime ?? new Date()).toISOString();
    const statuses = ['Todo', 'In progress', 'In review', 'Done'].map((name, i) => ({ id: `s${i}`, name, color: ['#8b8b93', '#d7a84b', '#7999e8', '#62b58d'][i] }));
    const board = { version: 1, status_revision: 1, statuses, inbox: [], tasks: [] };
    const writes = []; const reads = []; const receipts = new Map(); const failResponses = new Set(); let failCapture = false; let conflictMove = false;
    let idSequence = 2104775476131139584n;
    const nextId = () => (idSequence++).toString(36);
    await page.route('**/api/ids', route => {
      errors.push('Unexpected standalone ID allocation');
      return route.fulfill({ status: 404 });
    });
    await page.route('**/api/workspace/events', route => route.fulfill({ status: 200, contentType: 'text/event-stream', body: '' }));
    await page.route('**/api/aow/projects', route => route.fulfill({ json: [{ id: 'project', name: 'AoW', registered_path: '/repo', worktrees: [{ id: 'main', path: '/repo', branch: 'main' }] }] }));
    await page.route('**/api/tasks**', async route => {
      const request = route.request(); const url = new URL(request.url()); const path = url.pathname.replace('/api/tasks', '');
      if (request.method() === 'GET') {
        reads.push({ path, query: url.search });
        const project = url.searchParams.get('project_id');
        if (path.startsWith('/inbox/')) {
          const { task_ids, ...item } = board.inbox.find(item => item.id === path.split('/')[2]);
          return route.fulfill({ json: item });
        }
        if (path === '/inbox') {
          const include = url.searchParams.get('include_converted') === 'true';
          const rows = board.inbox.filter(item => (!project || item.project_id === project) && (include || !item.task_ids.length))
            .slice().sort((a, b) => b.created_at.localeCompare(a.created_at) || b.id.localeCompare(a.id));
          const total = rows.length; const cursor = url.searchParams.get('cursor');
          const offset = cursor ? rows.findIndex(item => item.id === cursor) + 1 : 0;
          const selected = rows.slice(offset, offset + Number(url.searchParams.get('limit') || 50));
          return route.fulfill({ json: { items: selected.map(({ description, ...item }) => item), total,
            next_cursor: offset + selected.length < total ? selected.at(-1).id : null } });
        }
        const { inbox, ...snapshot } = board;
        return route.fulfill({ json: { ...snapshot, tasks: board.tasks.filter(task => !project || task.project_id === project) } });
      }
      const input = request.postDataJSON(); writes.push({ path, input });
      const creation = path === '/inbox' || path === '/statuses' || path.endsWith('/convert');
      const receiptKey = `${path}:${input.request_key}`;
      if (creation) {
        assert.equal('id' in input, false, 'creation never supplies a resource ID');
        assert.equal(typeof input.request_key, 'string');
        const previous = receipts.get(receiptKey);
        if (previous) {
          assert.deepEqual(input, previous.input, 'retry keeps the same payload');
          return route.fulfill({ json: previous.result });
        }
      }
      const created = result => {
        receipts.set(receiptKey, { input, result: structuredClone(result) });
        if (failResponses.delete(path)) return route.fulfill({ status: 502, json: { message: '响应中断，请重试' } });
        return route.fulfill({ json: result });
      };
      if (path === '/inbox') {
        if (failCapture) return route.fulfill({ status: 500, json: { message: '保存失败，请重试' } });
        const { request_key, ...fields } = input;
        const item = { ...fields, id: nextId(), revision: 1, created_at: now, updated_at: now, task_ids: [] };
        board.inbox.unshift(item); return created(item);
      }
      if (/^\/inbox\/[^/]+$/.test(path)) {
        assert.equal('id' in input, false);
        const existing = board.inbox.find(item => item.id === path.split('/')[2]);
        if (input.expected_revision !== existing.revision) return route.fulfill({ status: 409, json: { message: 'This item changed' } });
        Object.assign(existing, { title: input.title, description: input.description, revision: existing.revision + 1 });
        return route.fulfill({ json: existing });
      }
      if (path === '/statuses') {
        if (input.expected_revision !== board.status_revision) return route.fulfill({ status: 409, json: { message: 'Status configuration changed' } });
        board.statuses = input.statuses.map(status => ({ ...status, id: status.id ?? nextId() })); board.status_revision++;
        return created(board);
      }
      if (path.endsWith('/convert')) {
        const item = board.inbox.find(i => i.id === path.split('/')[2]);
        const { request_key, ...fields } = input;
        const id = nextId();
        const cwd = input.cwd || `${input.project_id === 'other' ? '/other' : '/repo'}-task-${id}`;
        const task = { ...fields, id, cwd, revision: 1, inbox_id: item.id, tab_id: 'agent-terminal', pane_id: 'pane', execution: input.start_now ? 'submitted' : 'ready', archived: false, error: null, created_at: now, updated_at: now, history: [] };
        board.tasks.push(task); item.task_ids.push(task.id);
        return created(task);
      }
      if (path.endsWith('/delete')) {
        board.inbox = board.inbox.filter(item => item.id !== path.split('/')[2]);
        return route.fulfill({ json: { ok: true } });
      }
      const task = board.tasks.find(t => t.id === path.split('/')[2]);
      if (path.endsWith('/status')) {
        if (conflictMove) return route.fulfill({ status: 409, json: { message: 'This item changed. Refresh it before trying again.' } });
        task.status_id = input.status_id;
      }
      if (path.endsWith('/start')) task.execution = 'submitted';
      if (path.endsWith('/archive')) task.archived = !task.archived;
      task.revision++; return route.fulfill({ json: task });
    });
    await page.goto(`${base}/tests/tasks-preview.html`);
    await page.getByRole('region', { name: 'Todo', exact: true }).waitFor();
    return { page, board, writes, reads, errors, failResponse: path => failResponses.add(path), failCapture: value => { failCapture = value; }, conflictMove: value => { conflictMove = value; } };
  }
  async function writeMarkdown(page, value) {
    const input = page.getByRole('textbox', { name: '需求内容' });
    await input.focus();
    await input.press('ControlOrMeta+A');
    await page.keyboard.insertText(value);
  }
  async function markdownValue(page) {
    return page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      return monaco.editor.getEditors().find(editor => editor.getDomNode()?.closest('.tasks-inbox-editor'))?.getValue();
    });
  }
  async function capture(page, title, again = false) {
    const dialog = page.getByRole('dialog', { name: '录入需求' });
    await writeMarkdown(page, title);
    await dialog.getByRole('button', { name: again ? '连续创建' : '创建', exact: true }).click();
  }
  await test('continuous capture stays focused, failure preserves input, creation closes without agents', async t => {
    const { page, board, writes, errors, failCapture } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click();
    await capture(page, '第一个想法\n\n  ', true);
    const dialog = page.getByRole('dialog', { name: '录入需求' });
    await dialog.getByRole('status').filter({ hasText: '已加入 Inbox' }).waitFor();
    assert.equal(await markdownValue(page), '');
    assert.equal(board.inbox[0].description, '');
    assert.equal(await dialog.getByRole('textbox', { name: '需求内容' }).evaluate(el => document.activeElement === el), true);
    failCapture(true); await capture(page, '失败后保留\n\n- 完整 Markdown', true);
    await dialog.getByRole('alert').waitFor();
    assert.equal(await markdownValue(page), '失败后保留\n\n- 完整 Markdown');
    failCapture(false); await dialog.getByRole('button', { name: '创建', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.equal(board.inbox.length, 2); assert.equal(board.tasks.length, 0);
    assert.ok(writes.every(w => w.path === '/inbox')); assert.deepEqual(errors, []);
    assert.ok(writes.every(w => !('id' in w.input)));
    assert.equal(writes.at(-1).input.request_key, writes.at(-2).input.request_key, 'failed saves reuse their request key');
    assert.notEqual(writes[0].input.request_key, writes[1].input.request_key, 'each creation has a new request key');
    assert.ok(board.inbox.every(item => /^[0-9a-z]{1,13}$/.test(item.id)));
  });
  await test('one Markdown input derives its title, preserves the body on edit and passes it to conversion', async t => {
    const { page, board, writes, errors } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click();
    const dialog = page.getByRole('dialog', { name: '录入需求' });
    const input = dialog.getByRole('textbox', { name: '需求内容' });
    await input.waitFor();
    assert.equal(await dialog.getByRole('textbox').count(), 1);
    for (const value of ['', ' \n\t\n　']) {
      await writeMarkdown(page, value);
      assert.equal(await dialog.getByRole('button', { name: '创建', exact: true }).isDisabled(), true);
      assert.equal(await dialog.getByRole('button', { name: '连续创建', exact: true }).isDisabled(), true);
    }
    await writeMarkdown(page, '支持导出报告');
    await input.press('End'); await input.press('Enter');
    assert.equal(writes.length, 0, 'Enter continues writing instead of submitting');
    assert.equal(await markdownValue(page), '支持导出报告\n');
    const body = '\n需要支持导出 **Markdown** 和 PDF。\n\n- [ ] 保留代码块\n  - 保留缩进\n\n```ts\nconst exportReport = true;\n```\n';
    await writeMarkdown(page, ` \n\t\n支持导出报告\n${body}`);
    await dialog.locator('.tasks-inbox-title').filter({ hasText: '支持导出报告' }).waitFor();
    const titleStyle = await dialog.locator('.tasks-inbox-title').first().evaluate(node => ({ fontSize: getComputedStyle(node).fontSize, fontWeight: getComputedStyle(node).fontWeight }));
    assert.equal(titleStyle.fontWeight, '700');
    await page.screenshot({ path: '/tmp/aow-inbox-markdown.png', fullPage: true });
    await dialog.getByRole('button', { name: '创建', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.equal(board.inbox[0].title, '支持导出报告');
    assert.equal(board.inbox[0].description, body);
    await page.getByRole('listitem').filter({ hasText: '支持导出报告' }).click();
    const edit = page.getByRole('dialog', { name: '编辑需求' });
    await edit.getByRole('textbox', { name: '需求内容' }).waitFor();
    assert.equal(await markdownValue(page), `支持导出报告\n${body}`);
    await writeMarkdown(page, `导出报告与图片\n${body}`);
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({ path: '/tmp/aow-inbox-markdown-mobile.png', fullPage: true });
    assert.ok(await edit.evaluate(node => node.scrollWidth <= node.clientWidth));
    await edit.getByRole('button', { name: '保存', exact: true }).click(); await edit.waitFor({ state: 'hidden' });
    assert.equal(board.inbox.length, 1); assert.equal(board.inbox[0].revision, 2);
    assert.equal(board.inbox[0].title, '导出报告与图片'); assert.equal(board.inbox[0].description, body);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.getByRole('button', { name: '导出报告与图片 需求操作' }).click(); await page.getByRole('menuitem', { name: '转为任务…' }).click();
    await page.getByRole('dialog', { name: '转为任务', exact: true }).waitFor();
    const conversion = page.getByRole('dialog', { name: '转为任务' });
    assert.equal(await conversion.getByLabel('任务名称').inputValue(), '导出报告与图片');
    assert.equal(await conversion.getByLabel('任务说明').inputValue(), body);
    assert.deepEqual(errors, []);
  });
  await test('editing a failed creation starts a new request without using a client resource ID', async t => {
    const { page, board, writes, errors, failCapture } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click();
    failCapture(true);
    await capture(page, '原来的内容');
    const dialog = page.getByRole('dialog', { name: '录入需求' });
    await dialog.getByRole('alert').waitFor();
    failCapture(false);
    await capture(page, '修改后的内容');
    await dialog.waitFor({ state: 'hidden' });
    assert.notEqual(writes[0].input.request_key, writes[1].input.request_key);
    assert.equal(board.inbox.length, 1);
    assert.equal(board.inbox[0].title, '修改后的内容');
    assert.deepEqual(errors, []);
  });
  await test('title follows removal and undo; overlong titles stay editable without losing Markdown', async t => {
    const { page, writes, errors } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click();
    const dialog = page.getByRole('dialog', { name: '录入需求' });
    const input = dialog.getByRole('textbox', { name: '需求内容' });
    await writeMarkdown(page, '旧标题\n新标题\n\n- 正文');
    const isMac = await page.evaluate(() => navigator.platform.startsWith('Mac'));
    await input.press(isMac ? 'Meta+ArrowUp' : 'Control+Home');
    await input.press(isMac ? 'Meta+Shift+ArrowRight' : 'Shift+End'); await input.press('Backspace');
    await dialog.locator('.tasks-inbox-title').filter({ hasText: '新标题' }).waitFor();
    await input.press('ControlOrMeta+Z');
    await dialog.locator('.tasks-inbox-title').filter({ hasText: '旧标题' }).waitFor();
    await page.setViewportSize({ width: 390, height: 844 });
    for (const wrappedTitle of ['支持导出报告并保留正文与图片'.repeat(5), 'ReportMarkdownPreview'.repeat(12)]) {
      await writeMarkdown(page, `${wrappedTitle}\n\n正文`);
      await page.screenshot({ path: '/tmp/aow-inbox-markdown-wrapped-title.png', fullPage: true });
      assert.ok(await dialog.locator('.tasks-inbox-editor').evaluate(node => {
        const right = node.getBoundingClientRect().right;
        return [...node.querySelectorAll('.tasks-inbox-title')].every(title => title.getBoundingClientRect().right <= right);
      }), 'title text wraps inside the editor on a narrow screen');
    }
    const long = `${'需求'.repeat(100)}\n\n保留正文`;
    await writeMarkdown(page, long);
    await dialog.getByRole('button', { name: '创建', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '标题过长' }).waitFor();
    assert.equal(writes.length, 0); assert.equal(await markdownValue(page), long);
    await writeMarkdown(page, '缩短标题\n\n保留正文');
    await dialog.getByRole('button', { name: '创建', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    assert.equal(writes[0].input.description, '\n保留正文');
    assert.deepEqual(errors, []);
  });
  await test('conversion, custom status movement and start are independent; original terminal remains linked', async t => {
    const { page, board, writes, errors, conflictMove } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click(); await capture(page, '实现任务看板');
    await page.getByRole('button', { name: '实现任务看板 需求操作' }).click();
    await page.getByRole('menuitem', { name: '转为任务…' }).click();
    await page.getByRole('dialog', { name: '转为任务', exact: true }).waitFor();
    const dialog = page.getByRole('dialog', { name: '转为任务' });
    assert.equal(await dialog.getByLabel('任务分组').count(), 0);
    assert.equal(await dialog.getByLabel('项目', { exact: true }).count(), 0);
    assert.deepEqual(await dialog.getByLabel('工作目录').locator('option').allTextContents(), ['main · /repo']);
    assert.equal(await dialog.getByLabel('立即执行').isChecked(), false);
    await dialog.getByLabel('初始状态').selectOption('s2');
    await dialog.getByRole('button', { name: '创建任务' }).click();
    await page.getByRole('button', { name: '开始执行：实现任务看板' }).waitFor();
    assert.equal(board.tasks[0].status_id, 's2'); assert.equal(board.tasks[0].agent, 'custom-codex');
    assert.equal(board.tasks[0].project_id, 'project');
    assert.equal('group_id' in board.tasks[0], false);
    await page.getByRole('combobox', { name: '任务状态：实现任务看板' }).selectOption('s3');
    await page.getByRole('region', { name: 'Done', exact: true }).getByRole('button', { name: '开始执行：实现任务看板' }).waitFor();
    assert.equal(board.tasks[0].execution, 'ready'); assert.equal(writes.filter(w => w.path.endsWith('/start')).length, 0);
    conflictMove(true); await page.getByRole('combobox', { name: '任务状态：实现任务看板' }).selectOption('s0');
    await page.getByRole('alert').filter({ hasText: 'This item changed' }).waitFor();
    assert.equal(board.tasks[0].status_id, 's3'); conflictMove(false);
    await page.getByRole('button', { name: '开始执行：实现任务看板' }).click();
    await page.getByText('已提交执行', { exact: true }).waitFor();
    assert.equal(board.tasks[0].status_id, 's3');
    await page.getByRole('button', { name: '实现任务看板', exact: true }).click();
    await page.getByText('Opened terminal agent-terminal').waitFor();
    await page.getByRole('button', { name: 'Inbox 显示选项' }).click();
    await page.getByRole('menuitemcheckbox', { name: '显示已转化' }).click();
    await page.getByText('已转换', { exact: true }).waitFor();
    await page.screenshot({ path: '/tmp/aow-tasks-board.png', fullPage: true });
    assert.deepEqual(errors, []);
  });
  await test('drag moves a card and an external status event updates the board without reopening it', async t => {
    const { page, board, writes, errors } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click(); await capture(page, '外部状态同步');
    await page.getByRole('listitem').filter({ hasText: '外部状态同步' }).click({ button: 'right' });
    await page.getByRole('menuitem', { name: '转为任务…' }).click();
    await page.getByRole('dialog', { name: '转为任务', exact: true }).waitFor();
    await page.getByRole('dialog', { name: '转为任务' }).getByRole('button', { name: '创建任务' }).click();
    await page.getByRole('button', { name: '开始执行：外部状态同步' }).waitFor();
    await page.locator('.tasks-card').dragTo(page.getByRole('region', { name: 'In progress', exact: true }));
    await page.getByRole('region', { name: 'In progress', exact: true }).getByRole('button', { name: '外部状态同步', exact: true }).waitFor();
    assert.equal(board.tasks[0].execution, 'ready');
    board.tasks[0].status_id = 's2'; board.tasks[0].revision = 50;
    await page.evaluate(() => window.emitTaskBoardChange(50));
    await page.getByRole('region', { name: 'In review', exact: true }).getByRole('button', { name: '外部状态同步', exact: true }).waitFor();
    assert.equal(writes.filter(w => w.path.endsWith('/status')).length, 1);
    assert.equal(writes.filter(w => w.path.endsWith('/start')).length, 0);
    assert.deepEqual(errors, []);
  });
  await test('Inbox menu filters converted ideas and creation ages refresh while the panel stays open', async t => {
    const clockTime = new Date('2026-09-28T08:00:00Z');
    const { page, board, errors } = await fixture(t, { clockTime });
    const inbox = page.getByRole('region', { name: 'Inbox', exact: true });
    const row = title => inbox.getByRole('listitem').filter({ hasText: title });
    assert.equal(await inbox.getByRole('textbox').count(), 0);
    assert.equal(await inbox.getByRole('checkbox').count(), 0);
    board.inbox = ['已经转换的需求，保留长主题以检查单行省略', '五分钟前的想法', '刚记录的想法'].map((title, index) => ({
      id: `idea-${index}`, project_id: 'project', revision: 1, title, description: '', task_ids: [],
      created_at: clockTime.toISOString(), updated_at: clockTime.toISOString(),
    }));
    board.inbox[0].task_ids = ['linked-task'];
    board.inbox[0].created_at = new Date(clockTime.getTime() - 135 * 60_000).toISOString();
    board.inbox[1].created_at = new Date(clockTime.getTime() - 5 * 60_000).toISOString();
    await page.evaluate(() => window.emitTaskBoardChange(10));
    await row('五分钟前的想法').getByText('5 分钟前', { exact: true }).waitFor();
    await row('刚记录的想法').getByText('未转换', { exact: true }).waitFor();
    assert.equal(await row('刚记录的想法').locator('time').textContent(), '刚刚');
    assert.equal(await inbox.getByRole('listitem').count(), 2);
    const options = page.getByRole('button', { name: 'Inbox 显示选项' });
    const converted = page.getByRole('menuitemcheckbox', { name: '显示已转化' });
    await options.click();
    assert.equal(await converted.getAttribute('aria-checked'), 'false');
    await converted.press('Enter');
    await row('已经转换的需求').getByText('已转换', { exact: true }).waitFor();
    assert.equal(await row('已经转换的需求').locator('time').textContent(), '2 小时 15 分钟前');
    assert.equal(await row('已经转换的需求').locator('time').getAttribute('datetime'), board.inbox[0].created_at);
    assert.match(await row('已经转换的需求').locator('time').getAttribute('title'), /^录入时间：/);
    assert.equal(await row('已经转换的需求').getByText('1 个任务').count(), 0);
    await page.clock.fastForward(60_000);
    await row('五分钟前的想法').getByText('6 分钟前', { exact: true }).waitFor();
    await row('已经转换的需求').getByText('2 小时 16 分钟前', { exact: true }).waitFor();
    await page.screenshot({ path: '/tmp/aow-tasks-inbox.png', fullPage: true });
    await options.click();
    assert.equal(await converted.getAttribute('aria-checked'), 'true');
    await converted.press('Escape');
    assert.equal(await options.evaluate(el => document.activeElement === el), true);
    assert.equal(await inbox.getByRole('listitem').count(), 3);
    await options.click(); await converted.click();
    assert.equal(await inbox.getByRole('listitem').count(), 2);
    assert.deepEqual(errors, []);
  });
  await test('Inbox pages summaries and fetches a complete body only when opened', async t => {
    const { page, board, reads, errors } = await fixture(t);
    board.inbox = Array.from({ length: 112 }, (_, index) => ({
      id: `bulk-${String(index).padStart(3, '0')}`, project_id: 'project', revision: 1,
      title: `需求 ${index}`, description: `## 正文 ${index}\n\n- 完整内容`, task_ids: index < 55 ? ['execution'] : [],
      created_at: new Date(Date.UTC(2026, 8, 29, 0, index)).toISOString(), updated_at: new Date().toISOString(),
    }));
    await page.evaluate(() => window.emitTaskBoardChange(99));
    const inbox = page.getByRole('region', { name: 'Inbox', exact: true });
    await page.waitForFunction(() => document.querySelectorAll('[aria-label="Inbox"] [role="listitem"]').length === 50);
    assert.equal(reads.filter(read => read.path.startsWith('/inbox/')).length, 0);
    await inbox.getByRole('button', { name: '加载更多需求' }).click();
    await page.waitForFunction(() => document.querySelectorAll('[aria-label="Inbox"] [role="listitem"]').length === 57);
    assert.equal(await inbox.getByRole('button', { name: '加载更多需求' }).count(), 0);
    assert.ok(reads.some(read => read.path === '/inbox' && read.query.includes('cursor=')));
    await inbox.getByRole('listitem').filter({ hasText: '需求 55' }).locator('.aow-list-row-open').click();
    const dialog = page.getByRole('dialog', { name: '编辑需求' });
    await dialog.getByRole('textbox', { name: '需求内容' }).waitFor();
    assert.equal(await markdownValue(page), '需求 55\n## 正文 55\n\n- 完整内容');
    assert.equal(reads.filter(read => read.path === '/inbox/bulk-055').length, 1);
    await dialog.getByRole('button', { name: '取消', exact: true }).click();
    const row = inbox.getByRole('listitem').filter({ hasText: '需求 55' });
    await row.click({ button: 'right' });
    await page.getByRole('menuitem', { name: '删除需求', exact: true }).click();
    await page.getByRole('dialog', { name: '删除需求', exact: true }).getByRole('button', { name: '删除', exact: true }).click();
    await page.waitForFunction(() => document.querySelectorAll('[aria-label="Inbox"] [role="listitem"]').length === 56);
    assert.deepEqual(errors, []);
  });
  await test('one shared status configuration controls columns and conversion choices', async t => {
    const { page, board, writes, errors, failResponse } = await fixture(t);
    assert.equal(await page.getByRole('button', { name: '新建任务分组' }).count(), 0);
    assert.equal(await page.getByRole('combobox', { name: '任务分组' }).count(), 0);
    await page.getByRole('button', { name: '配置任务状态' }).click();
    const dialog = page.getByRole('dialog', { name: '配置任务状态' });
    await dialog.getByLabel('状态名称 1', { exact: true }).fill('草图');
    await dialog.getByRole('button', { name: '添加状态' }).click(); await dialog.getByLabel('状态名称 5', { exact: true }).fill('待确认');
    await dialog.getByRole('button', { name: '上移状态 5' }).click();
    assert.equal(writes.length, 0, 'draft status rows stay local until saving');
    failResponse('/statuses');
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.getByRole('alert').filter({ hasText: '响应中断' }).waitFor();
    assert.equal(board.statuses.length, 5);
    const assignedId = board.statuses[3].id;
    await dialog.getByRole('button', { name: '保存', exact: true }).click();
    await dialog.waitFor({ state: 'hidden' });
    await page.getByRole('region', { name: '待确认', exact: true }).waitFor();
    assert.equal(board.status_revision, 2); assert.deepEqual(board.statuses.map(s => s.name), ['草图', 'In progress', 'In review', '待确认', 'Done']);
    assert.equal(writes[0].input.expected_revision, 1);
    assert.equal('id' in writes[0].input.statuses[3], false);
    assert.equal(board.statuses[3].id, assignedId);
    assert.deepEqual(writes[0].input, writes[1].input);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click(); await capture(page, '使用统一状态');
    await page.getByRole('button', { name: '使用统一状态 需求操作' }).click(); await page.getByRole('menuitem', { name: '转为任务…' }).click();
    await page.getByRole('dialog', { name: '转为任务', exact: true }).waitFor();
    const conversion = page.getByRole('dialog', { name: '转为任务' });
    assert.deepEqual(await conversion.getByLabel('初始状态').locator('option').allTextContents(), board.statuses.map(s => s.name));
    await conversion.getByLabel('初始状态').selectOption(board.statuses[3].id);
    await conversion.getByRole('button', { name: '创建任务' }).click();
    await page.getByRole('region', { name: '待确认', exact: true }).getByRole('button', { name: '使用统一状态', exact: true }).waitFor();
    assert.deepEqual(errors, []);
  });
  await test('lost creation responses reuse request keys and keep the returned resource IDs', async t => {
    const { page, board, writes, errors, failResponse } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click();
    failResponse('/inbox');
    await capture(page, '响应丢失后重试');
    const editor = page.getByRole('dialog', { name: '录入需求' });
    await editor.getByRole('alert').filter({ hasText: '响应中断' }).waitFor();
    assert.equal(board.inbox.length, 1);
    const inboxId = board.inbox[0].id;
    await editor.getByRole('button', { name: '创建', exact: true }).click();
    await editor.waitFor({ state: 'hidden' });
    assert.equal(board.inbox.length, 1);
    assert.equal(board.inbox[0].id, inboxId);
    assert.notEqual(inboxId, writes[0].input.request_key);
    assert.deepEqual(writes[0].input, writes[1].input);
    await page.getByRole('button', { name: '响应丢失后重试 需求操作' }).click();
    await page.getByRole('menuitem', { name: '转为任务…' }).click();
    const dialog = page.getByRole('dialog', { name: '转为任务', exact: true });
    failResponse(`/inbox/${inboxId}/convert`);
    await dialog.getByRole('button', { name: '创建任务' }).click();
    await dialog.getByRole('alert').filter({ hasText: '响应中断' }).waitFor();
    assert.equal(board.tasks.length, 1);
    const taskId = board.tasks[0].id;
    await dialog.getByRole('button', { name: '创建任务' }).click();
    await dialog.waitFor({ state: 'hidden' });
    await page.getByRole('button', { name: '开始执行：响应丢失后重试' }).waitFor();
    assert.equal(board.tasks.length, 1);
    assert.equal(board.tasks[0].id, taskId);
    assert.notEqual(taskId, writes[2].input.request_key);
    assert.deepEqual(writes[2].input, writes[3].input);
    assert.deepEqual(board.inbox[0].task_ids, [taskId]);
    assert.deepEqual(errors, []);
  });
  await test('Inbox and board follow the current project; conversion inherits its worktree context', async t => {
    const { page, board, writes, errors } = await fixture(t);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click(); await capture(page, 'AoW 需求');
    await page.getByRole('button', { name: 'AoW 需求 需求操作' }).click(); await page.getByRole('menuitem', { name: '转为任务…' }).click();
    await page.getByRole('dialog', { name: '转为任务', exact: true }).waitFor();
    await page.getByRole('dialog', { name: '转为任务' }).getByRole('button', { name: '创建任务' }).click();
    await page.getByRole('button', { name: '开始执行：AoW 需求' }).waitFor();
    await page.getByRole('button', { name: 'Other 项目', exact: true }).click();
    assert.equal(await page.locator('.tasks-card').count(), 0);
    assert.equal(await page.getByRole('region', { name: 'Inbox', exact: true }).getByRole('listitem').count(), 0);
    await page.getByRole('button', { name: '录入需求', exact: true }).first().click(); await capture(page, 'Other 需求');
    await page.getByRole('button', { name: 'Other 需求 需求操作' }).click(); await page.getByRole('menuitem', { name: '转为任务…' }).click();
    await page.getByRole('dialog', { name: '转为任务', exact: true }).waitFor();
    const dialog = page.getByRole('dialog', { name: '转为任务' });
    assert.equal(await dialog.getByLabel('项目', { exact: true }).count(), 0);
    assert.deepEqual(await dialog.getByLabel('工作目录').locator('option').allTextContents(), ['main · /other']);
    await dialog.getByLabel('创建新的 Worktree').check();
    await dialog.locator('fieldset:not([disabled])').waitFor();
    assert.equal(await dialog.getByLabel('Worktree 路径').inputValue(), '');
    assert.equal(await dialog.getByLabel('新分支', { exact: true }).inputValue(), '');
    assert.equal(await dialog.getByLabel('基于分支 / 引用').inputValue(), 'main');
    await page.screenshot({ path: '/tmp/aow-task-convert-project.png', fullPage: true });
    await dialog.getByRole('button', { name: '创建任务' }).click();
    await page.getByRole('button', { name: '开始执行：Other 需求' }).waitFor();
    assert.deepEqual(board.inbox.map(i => i.project_id), ['other', 'project']);
    const converted = writes.filter(w => w.path.endsWith('/convert')).map(w => w.input);
    assert.deepEqual(converted.map(t => t.project_id), ['project', 'other']);
    assert.equal(converted[1].cwd, '');
    assert.equal(converted[1].worktree.branch, '');
    assert.equal(converted[1].worktree.base_ref, 'main');
    assert.equal(board.tasks[1].cwd, `/other-task-${board.tasks[1].id}`);
    await page.getByRole('button', { name: 'AoW 项目', exact: true }).click();
    await page.getByRole('button', { name: '开始执行：AoW 需求' }).waitFor();
    assert.equal(await page.getByRole('button', { name: '开始执行：Other 需求' }).count(), 0);
    await page.getByRole('button', { name: 'Inbox 显示选项' }).click(); await page.getByRole('menuitemcheckbox', { name: '显示已转化' }).click();
    await page.getByRole('button', { name: 'AoW 需求 需求操作' }).waitFor();
    assert.equal(await page.getByRole('button', { name: 'Other 需求 需求操作' }).count(), 0);
    assert.deepEqual(errors, []);
  });
} finally { await browser?.close(); await server.close(); }
