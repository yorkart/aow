import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { mkdir } from 'node:fs/promises';
import { createServer } from 'vite';
import { chromium } from 'playwright';
import { installLiveEvents } from './fixtures/live-events.mjs';
import { fillInboxEditor, inboxEditorState, inboxEditorValue } from './fixtures/inbox-editor.mjs';
import { composeOnWrappedLine, wrappedMarkdown } from './fixtures/editor-ime.mjs';
const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({ headless: true, ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const labels = [{ id: 'todo', name: 'TODO', color: '#94a3b8' }, { id: 'progress', name: 'InProgress', color: '#60a5fa' }, { id: 'review', name: 'Review', color: '#c4b5fd' }, { id: 'done', name: 'Done', color: '#86cfa5' }];
  const item = (id, markdown, project = null, tags = []) => ({ id, markdown, project_id: project, label_ids: tags, revision: 1, created_at: '', updated_at: '' });
  async function eventually(check) {
    const until = Date.now() + 8000;
    while (!check()) { assert.ok(Date.now() < until, 'expected persisted state'); await new Promise(resolve => setTimeout(resolve, 20)); }
  }
  async function chooseProject(page, trigger, name) {
    await trigger.click();
    const menu = page.getByRole('listbox', { name: await trigger.getAttribute('aria-label'), exact: true });
    assert.equal(await menu.getByRole('option', { selected: true }).count(), 1, 'project selection is single-choice');
    await menu.getByRole('option', { name, exact: true }).click();
    await menu.waitFor({ state: 'detached' });
  }
  async function fixture(t, { items = [], executions = [], comments = {}, width = 1000, userAgent, floating = false, labelList = labels, branchFailure = false, branches = { branches: ['feature/test', 'main', 'origin/main', 'origin/release'], default_branch: 'origin/main' } } = {}) {
    const context = await browser.newContext({ viewport: { width, height: 720 }, userAgent });
    // Remote HTTP origins do not expose randomUUID; all user actions must still work.
    await context.addInitScript(() => Object.defineProperty(crypto, 'randomUUID', { value: undefined }));
    if (floating) await context.addInitScript(() => {
      localStorage.setItem('aow-floating-pinned', 'true');
      if (!localStorage.getItem('aow-floating-active')) localStorage.setItem('aow-floating-active', JSON.stringify('inbox'));
      localStorage.setItem('aow-floating-open', 'true');
    });
    await installLiveEvents(context);
    if (floating) await context.routeWebSocket('**/api/terminals/**', socket => {
      socket.onMessage(message => {
        if (typeof message !== 'string' || JSON.parse(message).type !== 'claim') return;
        socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
        socket.send(JSON.stringify({ type: 'stream', epoch: 'fixture', offset: 0, reset: true, replay_bytes: 0,
          restore_cols: 80, restore_rows: 24, restore: 'Temporary Agent' }));
      });
    });
    const page = await context.newPage(); page.setDefaultTimeout(8000);
    const state = { revision: 0, labels: structuredClone(labelList), items: structuredClone(items), executions: structuredClone(executions), comments: structuredClone(comments), commentRequests: 0, commentFailure: false, requests: [], errors: [], captures: new Map(), fail: false, conflict: false, hold: undefined, captureLost: false, branchFailure, branchRequests: 0 };
    page.on('pageerror', error => state.errors.push(error.message));
    t.after(async () => { await context.close(); assert.deepEqual(state.errors, []); });
    await context.route('**/api/aow/projects/*/branches', async route => {
      state.branchRequests++;
      if (state.branchFailure) { state.branchFailure = false; return route.fulfill({ status: 503, json: { message: '暂时无法加载分支' } }); }
      return route.fulfill({ json: branches });
    });
    await context.route('**/api/inbox**', async route => {
      const request = route.request(), path = new URL(request.url()).pathname, method = request.method();
      if (method === 'GET' && path.endsWith('/comments')) {
        state.commentRequests++;
        if (state.commentFailure) { state.commentFailure = false; return route.fulfill({ status: 503, json: { message: '暂时无法加载评论' } }); }
        return route.fulfill({ json: state.comments[path.split('/')[4]] ?? [] });
      }
      if (method === 'GET') return route.fulfill({ json: { revision: state.revision, items: state.items, labels: state.labels, executions: state.executions, comment_counts: Object.fromEntries(state.items.map(item => [item.id, state.comments[item.id]?.length ?? 0])) } });
      const body = request.postDataJSON(); state.requests.push({ method, path, body });
      if (state.hold?.method === method) { const hold = state.hold; state.hold = undefined; await hold.promise; }
      if (state.fail) { state.fail = false; return route.fulfill({ status: 503, json: { message: '暂时无法保存' } }); }
      if (state.conflict) { state.conflict = false; return route.fulfill({ status: 409, json: { message: '内容已有更新' } }); }
      if (path === '/api/inbox/items') {
        const previous = state.captures.get(body.request_key);
        if (previous) assert.equal(body.markdown, previous.markdown, 'capture retries keep their original payload');
        const created = previous ? state.items.find(item => item.id === previous.id) : item(`item-${state.items.length}`, body.markdown);
        if (!previous) { state.captures.set(body.request_key, { id: created.id, markdown: body.markdown }); state.items.push(created); state.revision++; }
        if (state.captureLost) { state.captureLost = false; return route.fulfill({ status: 503, json: { message: '暂时无法保存' } }); }
        return route.fulfill({ json: created });
      }
      if (path === '/api/inbox/order') {
        assert.equal(body.expected_revision, state.revision);
        const from = state.items.findIndex(item => item.id === body.item_id); const [moved] = state.items.splice(from, 1);
        const to = body.before_id ? state.items.findIndex(item => item.id === body.before_id) : state.items.length;
        state.items.splice(to, 0, moved); state.revision++;
        return route.fulfill({ status: 204 });
      }
      if (path === '/api/inbox/labels') {
        state.labels = body.labels; state.revision++;
        state.items.forEach(item => { item.label_ids = item.label_ids.filter(id => state.labels.some(label => label.id === id)); });
        return route.fulfill({ status: 204 });
      }
      const id = path.split('/')[4]; const current = state.items.find(item => item.id === id);
      if (path.endsWith('/execute')) {
        assert.ok(current.project_id); assert.equal(body.expected_revision, current.revision);
        const run = { id: 'execution-one', item_id: id, agent: body.agent, phase: 'submitted', tab_id: 'terminal-one', pane_id: 'pane-one', error: null, markdown: body.append_prompt ? `${current.markdown}\n\n${body.append_prompt}` : current.markdown };
        state.executions = [run]; return route.fulfill({ status: 202, json: run });
      }
      assert.equal(body.expected_revision, current.revision);
      if (method === 'DELETE') {
        state.items = state.items.filter(item => item.id !== id); state.revision++;
        if (state.deleteLost) { state.deleteLost = false; return route.fulfill({ status: 503, json: { message: '响应丢失' } }); }
        return route.fulfill({ status: 204 });
      }
      Object.assign(current, body, { revision: current.revision + 1 }); state.revision++;
      return route.fulfill({ json: current });
    });
    await page.goto(`${base}/tests/inbox-preview.html${floating ? "?floating" : ""}`);
    await page.getByRole('button', { name: '添加需求', exact: true }).waitFor();
    await page.waitForFunction(() => !document.querySelector('.inbox-add-bar button').disabled);
    return { page, state };
  }

  for (const width of [1000, 390, 320]) await test(`comments expand inline in append order and render read-only Markdown at ${width}px`, async t => {
    const comments = [
      { id: 'first', author: { type: 'human', name: '用户' }, created_at: '2026-10-02T01:00:00Z', content: '## 原始补充\n\n**重点**\n\n- 第一项\n- 第二项\n\n1. 有序步骤\n2. 验证结果\n\n- [ ] 待办' },
      { id: 'second', author: { type: 'ai', name: 'Codex' }, created_at: '2026-10-02T02:00:00Z', content: '实现结论\n\n```rust\nfn main() {}\n```\n\n<script>window.inboxInjected=true</script><img src=x onerror="window.inboxInjected=true"><button>不可操作</button>' },
    ];
    const { page, state } = await fixture(t, { width, items: [item('a', '# 有评论的需求', 'project-0', ['todo', 'progress', 'review', 'done']), item('b', '# 没有评论')], comments: { a: comments } });
    const row = page.locator('[data-inbox-id="a"]'), emptyRow = page.locator('[data-inbox-id="b"]');
    const trigger = row.getByRole('button', { name: '评论', exact: true });
    assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
    assert.equal(await emptyRow.getByRole('button', { name: '评论', exact: true }).count(), 0);
    assert.equal(state.commentRequests, 0, 'comments load only when expanded');
    assert.deepEqual(await row.locator('.inbox-row-actions > button').evaluateAll(buttons => buttons.map(button => button.getAttribute('aria-label'))), ['评论', '执行需求', '删除需求']);
    await trigger.click();
    const section = row.getByRole('region', { name: '需求评论', exact: true });
    await section.locator(':scope > ol > li').nth(1).waitFor();
    assert.equal(await trigger.getAttribute('aria-expanded'), 'true');
    assert.deepEqual(await section.locator(':scope > ol > li').evaluateAll(items => items.map(item => item.dataset.commentId)), ['first', 'second']);
    assert.equal(await section.locator('strong').innerText(), '重点');
    assert.equal(await section.locator('.inbox-markdown li').first().evaluate(element => getComputedStyle(element).borderBottomWidth), '0px', 'Markdown list items do not inherit comment separators');
    assert.equal(await section.locator('.inbox-markdown ol').evaluate(element => getComputedStyle(element).listStyleType), 'decimal', 'Markdown ordered lists keep their numbering');
    assert.equal(await section.locator('pre code').innerText(), 'fn main() {}\n');
    assert.equal(await section.getByRole('checkbox').isDisabled(), true);
    assert.equal(await section.locator('button, textarea, [contenteditable]').count(), 0);
    assert.equal(await page.evaluate(() => window.inboxInjected), undefined);
    assert.equal(await page.getByRole('dialog').count(), 0);
    await section.locator('strong').dblclick();
    assert.equal(await row.getByRole('textbox').count(), 0, 'comments cannot enter the requirement editor');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    assert.equal(await row.evaluate(element => element.scrollWidth <= element.clientWidth), true);
    state.comments.a.push({ id: 'third', author: { type: 'ai', name: 'Codex' }, created_at: '2026-10-02T03:00:00Z', content: '最后的验证结果' });
    await page.evaluate(() => window.emitLiveEvent('workspace', { boot_id: 'watch-fixture', revision: 1, projects: 0, terminals: 0, inbox: 1, repositories: {} }));
    await section.getByText('最后的验证结果').waitFor();
    assert.equal(await section.locator(':scope > ol > li').count(), 3);
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-comments'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-comments-${width}.png` });
    await trigger.click();
    await section.waitFor({ state: 'detached' });
    assert.equal(await trigger.getAttribute('aria-expanded'), 'false');
    assert.equal(state.requests.length, 0, 'reading comments does not change requirements');
  });

  await test('comment loading errors can be retried inline', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '需求')], comments: { a: [{ id: 'one', author: { type: 'human', name: '用户' }, created_at: '2026-10-02T01:00:00Z', content: '补充说明' }] } });
    state.commentFailure = true;
    const row = page.locator('[data-inbox-id="a"]');
    await row.getByRole('button', { name: '评论', exact: true }).click();
    await row.getByRole('alert').waitFor();
    await row.getByRole('button', { name: '重试加载评论', exact: true }).click();
    await row.getByText('补充说明', { exact: true }).waitFor();
    assert.equal(await row.getByRole('alert').count(), 0);
  });

  await test('wrapped Markdown composition does not repeat lines, jump, or autosave unfinished IME text', async t => {
    const { page, state } = await fixture(t, { items: [item('ime', wrappedMarkdown)] });
    const row = page.locator('[data-inbox-id="ime"]');
    await row.locator('.inbox-edit-target').dblclick();
    const input = row.getByRole('textbox', { name: '需求 Markdown' });
    await composeOnWrappedLine(input, { whileComposing: async () => {
      await page.waitForTimeout(450);
      assert.equal(state.requests.length, 0, 'only confirmed Chinese text is saved');
      await input.dispatchEvent('keydown', { key: 'Enter', metaKey: true });
      assert.equal(await input.isVisible(), true, 'Monaco composition state suppresses completion even without a DOM composition event');
      const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-ime'; await mkdir(screenshots, { recursive: true });
      await page.screenshot({ path: `${screenshots}/inbox-composition.png` });
    } });
    await eventually(() => state.items[0].markdown === wrappedMarkdown.replace('prompt的', 'prompt的的'));
    await input.press('Meta+Enter');
    await row.locator('.inbox-markdown').waitFor();
  });

  await test('capture and editing autosave while typing, then render Markdown on shortcut or outside click', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const markdown = '# 导出报告\n\n支持 **Markdown** 和 `代码`\n\n- [ ] 保留数据';
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    await fillInboxEditor(input, markdown);
    const captureHint = page.locator('.inbox-new-row footer .inbox-editor-shortcut');
    assert.equal(await captureHint.isVisible(), true);
    assert.match(await captureHint.textContent(), /^(⌘|Ctrl) \+ Enter 提交$/);
    const captureFooter = await page.locator('.inbox-new-row footer').boundingBox();
    const captureHintBounds = await captureHint.boundingBox();
    assert.ok(Math.abs(captureHintBounds.x + captureHintBounds.width - captureFooter.x - captureFooter.width) <= 1, 'capture shortcut is on the right of the footer');
    await eventually(() => state.items.length === 1);
    assert.equal(await input.isVisible(), true, 'autosaving does not close the editor');
    assert.equal(await page.getByRole('button', { name: /^(提交|保存|取消)$/ }).count(), 0);
    assert.deepEqual(Object.keys(state.requests[0].body).sort(), ['markdown', 'request_key']);
    await fillInboxEditor(input, markdown + '\n继续补充');
    await eventually(() => state.items[0].markdown.endsWith('继续补充'));
    assert.equal(state.requests.filter(request => request.method === 'POST').length, 1);
    await input.press('Meta+Enter');
    const row = page.locator('[data-inbox-id="item-0"]'); await row.locator('.inbox-markdown').waitFor();
    assert.equal(await page.locator('.inbox-editor-shortcut').count(), 0, 'preview hides the editing shortcut');
    assert.equal(await row.locator('strong').innerText(), 'Markdown');
    assert.equal(await row.getByRole('button', { name: '执行需求', exact: true }).isDisabled(), true);
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(row.getByRole('textbox'), '修改后的第一行\n\n正文');
    await page.getByRole('textbox', { name: '搜索需求', exact: true }).click();
    await row.getByRole('textbox').waitFor({ state: 'detached' });
    await eventually(() => state.items[0].markdown === '修改后的第一行\n\n正文');
    assert.equal(await row.locator('.inbox-markdown').innerText(), '修改后的第一行\n\n正文');
    assert.equal(await page.getByRole('textbox', { name: '搜索需求', exact: true }).evaluate(element => element === document.activeElement), true);
  });

  for (const width of [1000, 390, 320]) await test(`inline editing preserves content alignment and shows the platform shortcut at ${width}px`, async t => {
    const markdown = '# 调整输入体验\n\n双击原文编辑，保留 **Markdown** 和 [文档](#docs) 链接。';
    const userAgent = width === 1000 ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' : width === 390 ? 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' : 'Mozilla/5.0 (X11; Linux x86_64)';
    const { page, state } = await fixture(t, { width, userAgent, items: [item('a', markdown, 'project-0', ['todo', 'progress', 'review', 'done'])] });
    const fill = async (input, text) => {
      await input.focus();
      await input.press(width === 1000 ? 'Meta+A' : 'Control+A');
      await page.keyboard.insertText(text);
    };
    const row = page.getByRole('article', { name: '调整输入体验' });
    assert.equal(await row.locator('.inbox-editor-shortcut').count(), 0);
    await row.locator('.inbox-markdown a').dblclick();
    assert.equal(await row.getByRole('textbox').count(), 0, 'links retain their interaction');
    assert.equal(await row.getByRole('button', { name: '编辑需求', exact: true }).count(), 0);
    assert.equal(await row.getByRole('button', { name: '设置标签', exact: true }).count(), 0);
    const controls = [row, row.locator('.inbox-content'), row.locator('.inbox-row-meta'), row.locator('.inbox-row-actions'), row.getByLabel('绑定项目'), row.getByRole('button', { name: '选择标签', exact: true })];
    const before = await Promise.all(controls.map(control => control.boundingBox()));
    await row.locator('.inbox-edit-target').dblclick();
    const input = row.getByRole('textbox', { name: '需求 Markdown', exact: true });
    assert.equal(await inboxEditorValue(input), markdown);
    const after = await Promise.all(controls.map(control => control.boundingBox()));
    const growth = after[1].height - before[1].height;
    const toolbarGrowth = after[2].height - before[2].height;
    assert.ok(growth >= -1, 'editing retains at least the rendered content height');
    assert.ok(Math.abs(after[0].height - before[0].height - growth - toolbarGrowth) <= 1, 'only content and a wrapping toolbar may grow');
    for (let i = 0; i <= 2; i++) for (const key of ['x', 'width']) {
      assert.ok(Math.abs(before[i][key] - after[i][key]) <= 1, `control ${i} ${key} remains stable`);
    }
    assert.ok(Math.abs(after[2].y - before[2].y - growth) <= 1, 'toolbar retains the same spacing below the content');
    for (const control of after.slice(3)) {
      assert.ok(control.x >= after[2].x && control.x + control.width <= after[2].x + after[2].width + 1, 'toolbar controls stay inside the content width');
      assert.ok(control.y >= after[2].y && control.y + control.height <= after[2].y + after[2].height + 1, 'toolbar controls stay inside the footer');
    }
    for (let i = 3; i < after.length; i++) for (let j = i + 1; j < after.length; j++) {
      assert.ok(after[i].x + after[i].width <= after[j].x + 1 || after[j].x + after[j].width <= after[i].x + 1
        || after[i].y + after[i].height <= after[j].y + 1 || after[j].y + after[j].height <= after[i].y + 1, 'toolbar controls do not overlap');
    }
    const hint = row.locator('.inbox-row-meta .inbox-editor-shortcut');
    assert.equal(await hint.isVisible(), true);
    assert.equal(await hint.textContent(), `${width === 1000 ? '⌘' : 'Ctrl'} + Enter 提交`);
    const hintBounds = await hint.boundingBox();
    assert.ok(Math.abs(hintBounds.x + hintBounds.width - after[2].x - after[2].width) <= 1, 'editing shortcut is on the right of the footer');
    assert.equal(await row.evaluate(element => element.scrollWidth <= element.clientWidth), true);
    const editorState = await inboxEditorState(input);
    assert.ok(editorState.contentHeight <= editorState.layout.height + 1, 'the full Markdown source remains visible');
    assert.equal(editorState.layout.contentLeft, 0, 'line-number and glyph gutters take no space');
    assert.equal(editorState.language, 'markdown');
    assert.equal(editorState.minimap, false); assert.equal(editorState.glyphMargin, false); assert.equal(editorState.folding, false);
    assert.equal(editorState.wordWrap, 'on');
    assert.equal(await row.locator('.line-numbers:visible').count(), 0);
    const inputBounds = await row.locator('.inbox-code-editor').boundingBox();
    assert.equal(inputBounds.x, before[1].x);
    assert.equal(inputBounds.width, before[1].width);
    assert.deepEqual(await row.locator('.inbox-code-editor').evaluate(element => { const style = getComputedStyle(element); return [style.paddingLeft, style.paddingRight, style.borderLeftWidth, style.borderRightWidth, style.backgroundColor]; }), ['0px', '0px', '0px', '0px', 'rgba(0, 0, 0, 0)']);
    assert.equal(await row.locator('.inbox-editor footer').count(), 0, 'editing uses the existing toolbar');
    assert.equal(await row.getByRole('button', { name: /^(提交|保存|取消)$/ }).count(), 0);
    assert.equal(await row.getByRole('button', { name: '绑定项目', exact: true }).isEnabled(), true);
    assert.equal(await row.getByRole('button', { name: '选择标签', exact: true }).isEnabled(), true);
    assert.equal(await row.getByRole('button', { name: '执行需求', exact: true }).count(), 0);
    assert.equal(await row.getByRole('button', { name: '删除需求', exact: true }).count(), 0);
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-inline-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-edit-${width}.png` });
    await input.press('Escape');
    await row.locator('.inbox-edit-target').waitFor();
    assert.equal(await hint.count(), 0);
    assert.equal(state.items[0].markdown, markdown);
    assert.equal((await row.boundingBox()).height, before[0].height);
    await row.locator('.inbox-edit-target').press('Enter');
    await fill(input, '通过键盘提交');
    await input.press('Control+Enter');
    await page.getByRole('article', { name: '通过键盘提交' }).waitFor();
    assert.equal(state.items[0].markdown, '通过键盘提交');
    const plain = page.getByRole('article', { name: '通过键盘提交' });
    const plainBefore = await plain.locator('.inbox-content').boundingBox();
    await plain.locator('.inbox-edit-target').dblclick();
    const resizingInput = plain.getByRole('textbox', { name: '需求 Markdown', exact: true });
    await resizingInput.waitFor();
    await page.waitForFunction(height => Math.abs(document.querySelector('[data-inbox-id="a"] .inbox-content').getBoundingClientRect().height - height) <= 1, plainBefore.height);
    const plainAfter = await plain.locator('.inbox-content').boundingBox();
    assert.ok(Math.abs(plainAfter.height - plainBefore.height) <= 1, `plain text switches without changing content height: ${plainBefore.height} -> ${plainAfter.height}`);
    await fill(resizingInput, '缩窄浮动工作区后也要完整显示输入的内容。'.repeat(20));
    await page.locator('#root').evaluate(element => { element.style.width = '240px'; });
    await page.waitForFunction(async () => { const { monaco } = await import('/src/features/editor/monaco.ts'); const editor = monaco.editor.getEditors().find(instance => instance.getDomNode()?.closest('.inbox-editor-seamless')); return editor && editor.getLayoutInfo().width === editor.getDomNode().closest('.inbox-code-editor').clientWidth && editor.getContentHeight() <= editor.getLayoutInfo().height + 1; });
    assert.equal(await plain.locator('.inbox-editor-shortcut').isVisible(), true);
    const narrowMeta = await plain.locator('.inbox-row-meta').boundingBox();
    const narrowHint = await plain.locator('.inbox-editor-shortcut').boundingBox();
    assert.ok(narrowHint.x >= narrowMeta.x && narrowHint.x + narrowHint.width <= narrowMeta.x + narrowMeta.width + 1, 'editing shortcut fits after narrowing the panel');
    await resizingInput.press('Escape');
  });

  await test('the system Markdown editor highlights syntax, supports undo and releases its model when closed', async t => {
    const markdown = '# 高亮需求\n\n普通正文，**重点** 和 [文档](https://example.com)。\n\n```js\nconst ready = true;\n```';
    const { page, state } = await fixture(t, { items: [item('a', markdown)] });
    const row = page.locator('[data-inbox-id="a"]');
    await row.locator('.inbox-edit-target').dblclick();
    const input = row.getByRole('textbox', { name: '需求 Markdown', exact: true });
    await input.waitFor();
    await page.waitForFunction(() => new Set([...document.querySelectorAll('.inbox-code-editor .view-lines span')].filter(element => element.textContent.trim()).map(element => getComputedStyle(element).color)).size >= 3);
    assert.equal(await row.locator('.line-numbers:visible, .minimap:visible, .glyph-margin:visible').count(), 0);
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-monaco'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-markdown-highlighting.png` });
    await input.press('ControlOrMeta+A');
    await input.press('x');
    assert.equal(await inboxEditorValue(input), 'x');
    await input.press('ControlOrMeta+z');
    assert.equal(await inboxEditorValue(input), markdown);
    await input.press('ControlOrMeta+Shift+z');
    assert.equal(await inboxEditorValue(input), 'x');
    await fillInboxEditor(input, '# 修改后的高亮需求');
    await input.press('Meta+Enter');
    await eventually(() => state.items[0].markdown === '# 修改后的高亮需求');
    await page.waitForFunction(async () => { const { monaco } = await import('/src/features/editor/monaco.ts'); return monaco.editor.getModels().length === 0; });
    await row.locator('.inbox-edit-target').dblclick();
    assert.equal(await inboxEditorValue(input), '# 修改后的高亮需求');
    await input.press('Escape');
  });

  await test('agent execution configures workspace and appends prompt with an idempotent retry', async t => {
    const { page, state } = await fixture(t, { width: 320, items: [item('a', '直接选择 Agent 执行', 'project-0')] });
    const row = page.getByRole('article', { name: '直接选择 Agent 执行' });
    const trigger = row.getByRole('button', { name: '执行需求', exact: true });
    await trigger.click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    await panel.waitFor();
    assert.equal(await page.getByRole('menu', { name: '选择 Agent 执行', exact: true }).count(), 0);
    await panel.getByRole('combobox', { name: 'Agent', exact: true }).selectOption('codex');
    assert.equal(await panel.getByRole('button', { name: '新建 Worktree', exact: true }).getAttribute('aria-pressed'), 'true');
    assert.equal(await panel.getByRole('combobox', { name: '已有 Worktree', exact: true }).count(), 0);
    await panel.getByRole('button', { name: '已有 Worktree', exact: true }).click();
    assert.equal(await panel.getByRole('combobox', { name: '分支来自', exact: true }).count(), 0);
    const worktrees = panel.getByRole('combobox', { name: '已有 Worktree', exact: true });
    assert.equal(await worktrees.getByRole('option').nth(0).textContent(), '主仓库 · main · 0');
    assert.equal(await worktrees.getByRole('option').nth(1).textContent(), 'feature/test · feature');
    await worktrees.selectOption('/repo/0/feature');
    await panel.getByRole('textbox', { name: '追加 prompt', exact: true }).fill('先分析需求，再执行。');
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-inline-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-execute-panel.png` });
    state.fail = true;
    await panel.getByRole('button', { name: '开始执行', exact: true }).click();
    await panel.getByRole('alert').filter({ hasText: '暂时无法保存' }).waitFor();
    assert.equal(await panel.isVisible(), true);
    await panel.getByRole('button', { name: '开始执行', exact: true }).click();
    await row.getByRole('button', { name: '打开终端', exact: true }).waitFor();
    const requests = state.requests.filter(request => request.path.endsWith('/execute'));
    assert.equal(requests.length, 2);
    assert.equal(requests[0].body.request_key, requests[1].body.request_key);
    assert.equal(requests[0].body.workspace_mode, 'existing');
    assert.equal(requests[0].body.workspace_path, '/repo/0/feature');
    assert.equal(requests[0].body.base_branch, '');
    assert.equal(requests[0].body.append_prompt, '先分析需求，再执行。');
    assert.deepEqual(requests[0].body, requests[1].body);
    assert.equal(state.executions[0].markdown, '直接选择 Agent 执行\n\n先分析需求，再执行。');
    assert.equal(state.executions.length, 1);
  });

  for (const width of [1000, 320]) await test(`submitted Agent icons open the latest terminal without an extra status row at ${width}px`, async t => {
    const { page } = await fixture(t, {
      width, items: [item('a', '继续执行需求', 'project-0', ['todo', 'progress', 'review', 'done'])],
      executions: [{ id: 'previous', item_id: 'a', agent: 'custom-codex', phase: 'submitted', tab_id: 'previous-terminal', error: null }],
    });
    const row = page.getByRole('article', { name: '继续执行需求' });
    const terminal = row.getByRole('button', { name: '打开终端', exact: true });
    assert.equal(await terminal.innerText(), '', 'terminal access uses only the Agent icon');
    assert.equal(await terminal.getAttribute('title'), '打开 Custom Codex 终端');
    assert.match(await terminal.locator('.agent-icon').getAttribute('src'), /codex\.png/);
    assert.equal(await row.locator('.inbox-execution').count(), 0);
    assert.equal(await row.getByText(/^已交给 /).count(), 0);
    assert.deepEqual(await row.locator('.inbox-row-actions button').evaluateAll(buttons => buttons.map(button => button.getAttribute('aria-label'))), ['打开终端', '执行需求', '删除需求']);
    const meta = await row.locator('.inbox-row-meta').boundingBox();
    const controls = await Promise.all(['绑定项目', '选择标签', '打开终端', '执行需求', '删除需求'].map(name => row.getByRole('button', { name, exact: true }).boundingBox()));
    for (const control of controls) {
      assert.ok(control.x >= meta.x && control.x + control.width <= meta.x + meta.width + 1, 'controls remain inside the row');
      assert.ok(control.y >= meta.y && control.y + control.height <= meta.y + meta.height + 1, 'controls share one action row');
    }
    for (let index = 1; index < controls.length; index++) assert.ok(controls[index].x >= controls[index - 1].x + controls[index - 1].width, 'controls do not overlap');
    await terminal.click();
    assert.equal(await page.locator('output').getAttribute('data-opened-terminal'), 'previous-terminal');
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-inline-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-agent-icon-${width}.png` });
    await row.locator('.inbox-edit-target').dblclick();
    await row.getByRole('textbox', { name: '需求 Markdown', exact: true }).waitFor();
    assert.equal(await terminal.isVisible(), true, 'editing keeps the terminal accessible');
    const editingControls = await row.locator('.inbox-row-actions').boundingBox();
    const editingMeta = await row.locator('.inbox-row-meta').boundingBox();
    assert.ok(editingControls.x >= editingMeta.x && editingControls.x + editingControls.width <= editingMeta.x + editingMeta.width + 1, 'terminal, save status and shortcut stay inside the footer');
    assert.equal(await row.locator('.inbox-editor-shortcut').isVisible(), true);
    await row.getByRole('textbox', { name: '需求 Markdown', exact: true }).press('Escape');
    await row.getByRole('button', { name: '执行需求', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    await panel.getByRole('button', { name: '已有 Worktree', exact: true }).click();
    await panel.getByRole('button', { name: '开始执行', exact: true }).click();
    await page.waitForFunction(() => document.querySelector('.inbox-row-actions [aria-label="打开终端"]')?.title === '打开 Codex 终端');
    await terminal.click();
    assert.equal(await page.locator('output').getAttribute('data-opened-terminal'), 'terminal-one');
    assert.equal(await row.locator('.inbox-execution').count(), 0);
  });

  await test('starting stays in the execute button while failures and interrupted terminals remain accessible', async t => {
    const { page, state } = await fixture(t, {
      items: [item('a', '执行状态反馈', 'project-0')],
      executions: [{ id: 'starting', item_id: 'a', agent: 'codex', phase: 'starting', tab_id: null, error: null }],
    });
    const row = page.getByRole('article', { name: '执行状态反馈' });
    const execute = row.getByRole('button', { name: '执行需求', exact: true });
    assert.equal(await execute.isDisabled(), true);
    assert.equal(await execute.getAttribute('title'), '正在启动 Agent…');
    assert.equal(await execute.locator('.inbox-spin').count(), 1);
    assert.equal(await row.locator('.inbox-execution').count(), 0);
    assert.equal(await row.getByRole('button', { name: '打开终端', exact: true }).count(), 0);
    state.executions[0].phase = 'failed'; state.executions[0].error = 'Agent 启动失败';
    await page.reload();
    await row.getByRole('alert').filter({ hasText: 'Agent 启动失败' }).waitFor();
    assert.equal(await execute.isEnabled(), true);
    state.executions[0].phase = 'interrupted'; state.executions[0].tab_id = 'interrupted-terminal'; state.executions[0].error = '执行已中断';
    await page.reload();
    await row.getByRole('alert').filter({ hasText: '执行已中断' }).waitFor();
    await row.getByRole('button', { name: '打开终端', exact: true }).click();
    assert.equal(await page.locator('output').getAttribute('data-opened-terminal'), 'interrupted-terminal');
  });

  await test('new Worktree selects a remote base branch and preserves each mode selection', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '从指定分支新建工作区', 'project-0')] });
    await page.getByRole('button', { name: '执行需求', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    const branch = panel.getByRole('combobox', { name: '分支来自', exact: true });
    await branch.getByRole('option', { name: 'origin/main（默认）', exact: true }).waitFor({ state: 'attached' });
    assert.equal(await branch.inputValue(), 'origin/main');
    await branch.selectOption('origin/release');
    await panel.getByRole('button', { name: '已有 Worktree', exact: true }).click();
    const worktree = panel.getByRole('combobox', { name: '已有 Worktree', exact: true });
    await worktree.selectOption('/repo/0/feature');
    await panel.getByRole('button', { name: '新建 Worktree', exact: true }).click();
    assert.equal(await branch.inputValue(), 'origin/release');
    await panel.getByRole('button', { name: '已有 Worktree', exact: true }).click();
    assert.equal(await worktree.inputValue(), '/repo/0/feature');
    await panel.getByRole('button', { name: '新建 Worktree', exact: true }).click();
    assert.equal(state.branchRequests, 1, 'switching modes reuses the loaded branches');
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-inline-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-execute-new-worktree.png` });
    await panel.getByRole('button', { name: '开始执行', exact: true }).click();
    await page.getByRole('button', { name: '打开终端', exact: true }).waitFor();
    const request = state.requests.find(request => request.path.endsWith('/execute'));
    assert.equal(request.body.workspace_mode, 'new_worktree');
    assert.equal(request.body.workspace_path, '');
    assert.equal(request.body.base_branch, 'origin/release');
    assert.equal(state.executions[0].markdown, '从指定分支新建工作区');
  });

  await test('branch loading failures can retry and do not block existing Worktree execution', async t => {
    const { page, state } = await fixture(t, { branchFailure: true, items: [item('a', '分支查询失败后重试', 'project-0')] });
    await page.getByRole('button', { name: '执行需求', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    await panel.getByRole('alert').filter({ hasText: '暂时无法加载分支' }).waitFor();
    assert.equal(await panel.getByRole('button', { name: '开始执行', exact: true }).isDisabled(), true);
    await panel.getByRole('button', { name: '已有 Worktree', exact: true }).click();
    assert.equal(await panel.getByRole('button', { name: '开始执行', exact: true }).isEnabled(), true);
    await panel.getByRole('button', { name: '新建 Worktree', exact: true }).click();
    await panel.getByRole('button', { name: '重新加载', exact: true }).click();
    await panel.getByRole('combobox', { name: '分支来自', exact: true }).getByRole('option', { name: 'origin/main（默认）', exact: true }).waitFor({ state: 'attached' });
    assert.equal(await panel.getByRole('button', { name: '开始执行', exact: true }).isEnabled(), true);
    assert.equal(state.branchRequests, 2);
  });

  await test('temporary workspace execution needs no branch and retains the directory for its Agent', async t => {
    const { page, state } = await fixture(t, { width: 320, branchFailure: true, items: [item('a', '在临时工作区执行', 'project-0')] });
    await page.getByRole('button', { name: '执行需求', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    const choices = panel.getByRole('group', { name: '工作区方式', exact: true });
    assert.equal(await choices.getByRole('button').count(), 3);
    await choices.getByRole('button', { name: '动态工作区', exact: true }).click();
    assert.equal(await panel.getByRole('combobox').count(), 1, 'only the Agent selector remains');
    assert.equal(await panel.getByRole('radio', { name: '动态指定', exact: true }).isDisabled(), true);
    await panel.getByText('此工作区会保留供 Agent 使用，不会自动清理。', { exact: true }).waitFor();
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-inline-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-execute-temporary.png` });
    await panel.getByRole('button', { name: '开始执行', exact: true }).click();
    await page.getByRole('button', { name: '打开终端', exact: true }).waitFor();
    const request = state.requests.find(request => request.path.endsWith('/execute'));
    assert.equal(request.body.workspace_mode, 'temporary');
    assert.equal(request.body.workspace_path, '');
    assert.equal(request.body.base_branch, '');
  });

  await test('temporary Agent terminal opens in the floating workspace and survives project refresh and reload', async t => {
    const { page, state } = await fixture(t, { floating: true, items: [item('a', '打开临时 Agent', 'project-0')] });
    const cwd = '/private/tmp/aow-inbox-test';
    const pane = { id: 'pane-one', name: 'Codex', cwd, shell: '/bin/codex', kind: 'agent', agent_id: 'codex', status: 'interrupted', cols: 80, rows: 24, agent_terminal: { phase: 'interrupted', task_submitted: true } };
    const tab = { id: 'terminal-one', name: '临时 Agent', workspace_root: cwd, layout: { type: 'pane', pane_id: pane.id }, panes: [pane] };
    await page.route('**/api/terminals**', route => {
      const url = new URL(route.request().url());
      if (url.pathname === '/api/terminals/agents') return route.fulfill({ json: { agents: {}, titles: {}, processes: {} } });
      if (url.pathname === '/api/terminals') return route.fulfill({ json: { tabs: [tab] } });
      return route.fulfill({ json: tab });
    });
    await page.getByRole('button', { name: '执行需求', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    await panel.getByRole('button', { name: '动态工作区', exact: true }).click();
    await panel.getByRole('button', { name: '开始执行', exact: true }).click();
    await page.getByRole('button', { name: '打开终端', exact: true }).click();
    const terminal = page.locator('[data-floating-workspace] .terminal-pane');
    await terminal.waitFor();
    assert.equal(state.requests.find(request => request.path.endsWith('/execute')).body.workspace_mode, 'temporary');
    await page.getByTestId('refresh-projects').click();
    assert.equal(await terminal.isVisible(), true);
    await page.reload();
    await terminal.waitFor();
    assert.equal(await terminal.isVisible(), true);
    assert.equal(await page.locator('[data-floating-workspace] [role="tab"]').filter({ hasText: '临时 Agent' }).count(), 1);
  });

  await test('a project without branches cannot start a new Worktree', async t => {
    const { page } = await fixture(t, { branches: { branches: [], default_branch: '' }, items: [item('a', '没有可用分支', 'project-0')] });
    await page.getByRole('button', { name: '执行需求', exact: true }).click();
    const panel = page.getByRole('dialog', { name: '执行需求', exact: true });
    await panel.getByRole('combobox', { name: '分支来自', exact: true }).getByRole('option', { name: '当前项目没有可选分支', exact: true }).waitFor({ state: 'attached' });
    assert.equal(await panel.getByRole('button', { name: '开始执行', exact: true }).isDisabled(), true);
  });

  await test('a lost capture response and reload preserve the latest draft without duplicate requirements', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    state.captureLost = true;
    await fillInboxEditor(input, '第一次录入');
    await page.getByRole('alert').filter({ hasText: '暂时无法保存' }).waitFor();
    await fillInboxEditor(input, '失败之后继续补充的内容');
    await page.reload();
    assert.equal(await inboxEditorValue(input), '失败之后继续补充的内容');
    await eventually(() => state.items.length === 1 && state.items[0].markdown === '失败之后继续补充的内容');
    const captures = state.requests.filter(request => request.method === 'POST');
    assert.equal(captures.length, 2); assert.deepEqual(captures[0].body, captures[1].body);
    await input.press('Control+Enter');
    await page.getByRole('article', { name: '失败之后继续补充的内容' }).waitFor();
    assert.equal(await page.getByRole('article').count(), 1);
  });

  await test('projects and multiple labels autosave inside an editor without dismissing it', async t => {
    const { page, state } = await fixture(t, { width: 390, items: [item('a', '一起编辑需求', 'project-0', ['todo'])] });
    const row = page.locator('[data-inbox-id="a"]');
    const input = row.getByRole('textbox', { name: '需求 Markdown', exact: true });
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(input, '正文和项目标签实时保存');
    await chooseProject(page, row.getByRole('button', { name: '绑定项目', exact: true }), 'Website');
    await row.getByRole('button', { name: '选择标签', exact: true }).click();
    const picker = page.getByRole('dialog', { name: '选择需求标签', exact: true });
    await picker.getByRole('checkbox', { name: 'Review', exact: true }).check();
    await eventually(() => state.items[0].markdown === '正文和项目标签实时保存' && state.items[0].project_id === 'project-1' && state.items[0].label_ids.length === 2);
    assert.equal(await input.isVisible(), true);
    assert.equal(await picker.isVisible(), true);
    assert.equal(await picker.getByRole('checkbox', { checked: true }).count(), 2);
    await page.keyboard.press('Escape');
    assert.equal(await input.isVisible(), true, 'menu Escape only closes the menu');
    await chooseProject(page, row.getByRole('button', { name: '绑定项目', exact: true }), '未绑定项目');
    await input.press('Escape');
    await eventually(() => state.items[0].project_id === null);
    await row.getByRole('button', { name: '执行需求', exact: true }).waitFor();
    assert.equal(await row.getByRole('button', { name: '执行需求', exact: true }).isDisabled(), true);
    await page.evaluate(base => localStorage.setItem('aow-inbox-draft:a', JSON.stringify({ base, markdown: '旧版草稿' })), state.items[0]);
    await page.reload();
    assert.equal(await inboxEditorValue(input), '旧版草稿');
    await eventually(() => state.items[0].markdown === '旧版草稿');
    assert.equal(state.items[0].project_id, null); assert.deepEqual(state.items[0].label_ids, ['todo', 'review']);
    await input.press('Escape');
  });

  await test('typing during a slow save stays editable and writes the newest revision after closing', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '原始内容')] });
    let release;
    state.hold = { method: 'PUT', promise: new Promise(resolve => { release = resolve; }) };
    const row = page.locator('[data-inbox-id="a"]');
    await row.locator('.inbox-edit-target').dblclick();
    const input = row.getByRole('textbox');
    await fillInboxEditor(input, '第一轮自动保存');
    await eventually(() => state.requests.some(request => request.method === 'PUT'));
    assert.equal(await input.isEnabled(), true);
    await fillInboxEditor(input, '保存期间继续输入的最新内容');
    await input.press('Control+Enter');
    assert.equal(await row.getByRole('textbox').count(), 0, 'finish immediately renders, even while the request is pending');
    assert.equal(await row.locator('.inbox-markdown').innerText(), '保存期间继续输入的最新内容');
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(input, '重新打开后继续补充的内容');
    await input.press('Control+Enter');
    assert.equal(await row.getByRole('textbox').count(), 0);
    release();
    await eventually(() => state.items[0].markdown === '重新打开后继续补充的内容');
    assert.deepEqual(state.requests.filter(request => request.method === 'PUT').map(request => request.body.expected_revision), [1, 2]);
    await row.getByRole('button', { name: '删除需求', exact: true }).waitFor();
  });

  await test('slow initial capture keeps a single row and a later capture gets a fresh request key', async t => {
    const { page, state } = await fixture(t);
    let release;
    state.hold = { method: 'POST', promise: new Promise(resolve => { release = resolve; }) };
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    await fillInboxEditor(input, '第一段内容');
    await eventually(() => state.requests.length === 1);
    await fillInboxEditor(input, '第一条需求的最新内容');
    await input.press('Meta+Enter');
    assert.equal(await input.count(), 0);
    await page.getByText('第一条需求的最新内容', { exact: true }).waitFor();
    release();
    await page.getByRole('article', { name: '第一条需求的最新内容' }).waitFor();
    assert.equal(state.items.length, 1);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    await fillInboxEditor(input, '第二条需求');
    await input.press('Meta+Enter');
    await page.getByRole('article', { name: '第二条需求' }).waitFor();
    assert.equal(state.items.length, 2);
    const captures = state.requests.filter(request => request.method === 'POST');
    assert.notEqual(captures[0].body.request_key, captures[1].body.request_key);
  });

  await test('new requirements start with a title prefix and place typing after it without saving the placeholder', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    assert.equal(await inboxEditorValue(input), '## ');
    await page.waitForTimeout(450);
    assert.equal(state.requests.length, 0);
    await page.keyboard.insertText('直接输入标题');
    assert.equal(await inboxEditorValue(input), '## 直接输入标题');
    await input.press('Meta+Enter');
    const row = page.getByRole('article', { name: '直接输入标题' });
    await row.getByRole('heading', { level: 2, name: '直接输入标题' }).waitFor();
    assert.equal(state.items[0].markdown, '## 直接输入标题');
  });

  const emptyMarkdown = ['', ' \n\t\u00a0\u3000', '#', '## ', ' # # \n\t#\u00a0', '########'];
  await test('new empty or hash-only drafts ignore whitespace everywhere and disappear without a capture', async t => {
    const { page, state } = await fixture(t);
    for (const markdown of emptyMarkdown) {
      await page.getByRole('button', { name: '添加需求', exact: true }).click();
      const input = page.getByRole('textbox', { name: '新需求 Markdown' });
      await fillInboxEditor(input, markdown);
      await page.waitForTimeout(400);
      assert.equal(state.requests.length, 0, 'placeholder text is never autosaved');
      await page.getByRole('textbox', { name: '搜索需求', exact: true }).click();
      await page.locator('.inbox-new-row').waitFor({ state: 'detached' });
      assert.equal(await page.evaluate(() => localStorage.getItem('aow-inbox-draft:new')), null);
    }
    await page.reload();
    assert.equal(await page.locator('.inbox-new-row').count(), 0);
    assert.equal(state.items.length, 0);
  });

  await test('persisted requirements cleared to whitespace or hashes are removed only when editing ends', async t => {
    const { page, state } = await fixture(t, { items: emptyMarkdown.map((_, index) => item(`empty-${index}`, `原始需求 ${index}`)) });
    for (const [index, markdown] of emptyMarkdown.entries()) {
      const row = page.locator(`[data-inbox-id="empty-${index}"]`);
      await row.locator('.inbox-edit-target').dblclick();
      await fillInboxEditor(row.getByRole('textbox'), markdown);
      await page.waitForTimeout(400);
      assert.equal(state.items.length, emptyMarkdown.length - index, 'clearing during typing does not delete');
      if (index % 2) await page.getByRole('textbox', { name: '搜索需求', exact: true }).click();
      else await row.getByRole('textbox').press('Control+Enter');
      await row.waitFor({ state: 'detached' });
      assert.equal(await page.evaluate(key => localStorage.getItem(`aow-inbox-draft:${key}`), `empty-${index}`), null);
    }
    assert.deepEqual(state.requests.map(request => request.method), emptyMarkdown.map(() => 'DELETE'));
    await page.reload();
    await page.getByText('先把想法记下来', { exact: true }).waitFor();
  });

  for (const method of ['POST', 'PUT']) await test(`emptying while ${method} is pending removes the saved item with its confirmed revision`, async t => {
    const { page, state } = await fixture(t, { items: method === 'PUT' ? [item('a', '已有需求')] : [] });
    let release;
    state.hold = { method, promise: new Promise(resolve => { release = resolve; }) };
    try {
      if (method === 'POST') await page.getByRole('button', { name: '添加需求', exact: true }).click();
      else await page.locator('.inbox-edit-target').dblclick();
      const input = page.getByRole('textbox', { name: method === 'POST' ? '新需求 Markdown' : '需求 Markdown', exact: true });
      await fillInboxEditor(input, '正在自动保存的内容');
      await eventually(() => state.requests.some(request => request.method === method));
      await fillInboxEditor(input, ' # \n # ');
      await input.press('Meta+Enter');
      assert.equal(await input.count(), 0);
      assert.equal(state.requests.some(request => request.method === 'DELETE'), false, 'wait for the in-flight write');
      release();
      await eventually(() => state.requests.some(request => request.method === 'DELETE') && state.items.length === 0);
      await page.getByText('先把想法记下来', { exact: true }).waitFor();
      assert.deepEqual(state.requests.map(request => request.method), [method, 'DELETE']);
      assert.equal(state.requests.at(-1).body.expected_revision, method === 'POST' ? 1 : 2);
      await page.getByRole('button', { name: '添加需求', exact: true }).click();
      assert.equal(await inboxEditorValue(page.getByRole('textbox', { name: '新需求 Markdown' })), '## ');
    } finally { release(); }
  });

  await test('a lost capture response is recovered before removing an emptied draft', async t => {
    const { page, state } = await fixture(t);
    state.captureLost = true;
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    await fillInboxEditor(input, '已经创建但响应丢失');
    await page.locator('.inbox-new-row').getByRole('alert').waitFor();
    assert.equal(state.items.length, 1);
    await fillInboxEditor(input, '# #');
    await input.press('Escape');
    await page.getByText('先把想法记下来', { exact: true }).waitFor();
    assert.equal(state.items.length, 0);
    assert.deepEqual(state.requests.map(request => request.method), ['POST', 'POST', 'DELETE']);
    assert.deepEqual(state.requests[0].body, state.requests[1].body);
  });

  await test('failed removal retains its intent across reload and retries without saving an empty item', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    await fillInboxEditor(input, '已经自动保存的需求');
    await eventually(() => state.items.length === 1);
    state.fail = true;
    await fillInboxEditor(input, '');
    await input.press('Escape');
    await page.locator('.inbox-new-row').getByRole('alert').waitFor();
    assert.equal(state.items[0].markdown, '已经自动保存的需求');
    assert.equal(await input.count(), 0);
    await page.reload();
    await page.getByText('先把想法记下来', { exact: true }).waitFor();
    assert.equal(state.items.length, 0);
    assert.deepEqual(state.requests.map(request => request.method), ['POST', 'DELETE', 'DELETE']);
  });

  await test('a lost delete response is confirmed from the snapshot without leaving an empty draft', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    await fillInboxEditor(input, '自动保存后清空');
    await eventually(() => state.items.length === 1);
    state.deleteLost = true;
    await fillInboxEditor(input, '#');
    await input.press('Escape');
    await page.getByText('先把想法记下来', { exact: true }).waitFor();
    assert.equal(await page.getByRole('region', { name: 'Inbox', exact: true }).getByRole('alert').count(), 0);
    assert.equal(await page.evaluate(() => localStorage.getItem('aow-inbox-draft:new')), null);
    assert.deepEqual(state.requests.map(request => request.method), ['POST', 'DELETE']);
  });

  await test('clearing a requirement preserves a newer remote version until conflict resolution', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '原始内容')] });
    const row = page.locator('[data-inbox-id="a"]');
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(row.getByRole('textbox'), ' # # ');
    Object.assign(state.items[0], { markdown: '其他窗口补充的内容', revision: 2 }); state.revision++;
    await page.evaluate(() => window.emitLiveEvent('workspace', { boot_id: 'watch-fixture', revision: 1, projects: 0, terminals: 0, inbox: 1, repositories: {} }));
    await row.getByRole('button', { name: '载入最新内容' }).waitFor();
    await row.getByRole('textbox').press('Escape');
    await row.getByRole('button', { name: '按最新版本移除' }).waitFor();
    assert.equal(state.requests.length, 0);
    await row.getByRole('button', { name: '载入最新内容' }).click();
    await row.locator('.inbox-markdown').getByText('其他窗口补充的内容', { exact: true }).waitFor();
    assert.equal(state.items[0].markdown, '其他窗口补充的内容');
    assert.equal(state.requests.length, 0);
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(row.getByRole('textbox'), '#');
    Object.assign(state.items[0], { markdown: '再次更新', revision: 3 }); state.revision++;
    await page.evaluate(() => window.emitLiveEvent('workspace', { boot_id: 'watch-fixture', revision: 2, projects: 0, terminals: 0, inbox: 2, repositories: {} }));
    await row.getByRole('button', { name: '载入最新内容' }).waitFor();
    await row.getByRole('textbox').press('Escape');
    await row.getByRole('button', { name: '按最新版本移除' }).click();
    await row.waitFor({ state: 'detached' });
    assert.equal(state.requests.at(-1).body.expected_revision, 3);
  });

  await test('IME input does not create partial requirements and clearing the saved content removes it', async t => {
    const { page, state } = await fixture(t);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    const input = page.getByRole('textbox', { name: '新需求 Markdown' });
    await input.press('Escape');
    assert.equal(await input.count(), 0); assert.equal(state.requests.length, 0);
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    await input.focus();
    const ime = await page.context().newCDPSession(page);
    await ime.send('Input.imeSetComposition', { text: '中文输', selectionStart: 3, selectionEnd: 3 });
    await input.dispatchEvent('keydown', { key: 'Enter', metaKey: true, isComposing: true });
    await page.waitForTimeout(450);
    assert.equal(await input.isVisible(), true); assert.equal(state.requests.length, 0);
    await ime.send('Input.insertText', { text: '中文输入' });
    await ime.detach();
    await eventually(() => state.items.length === 1);
    await input.press('Escape');
    const row = page.getByRole('article', { name: '中文输入' }); await row.waitFor();
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(row.getByRole('textbox'), '');
    await row.getByRole('textbox').press('Escape');
    await row.waitFor({ state: 'detached' });
    assert.equal(state.items.length, 0);
    assert.equal(state.requests.at(-1).method, 'DELETE');
  });

  await test('editing a filtered requirement stays visible until editing finishes', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '项目内需求', 'project-0')] });
    await chooseProject(page, page.getByRole('button', { name: '按项目筛选', exact: true }), 'AoW');
    const row = page.locator('[data-inbox-id="a"]');
    await row.locator('.inbox-edit-target').dblclick();
    await chooseProject(page, row.getByRole('button', { name: '绑定项目', exact: true }), 'Website');
    await eventually(() => state.items[0].project_id === 'project-1');
    assert.equal(await row.getByRole('textbox').isVisible(), true);
    await row.getByRole('button', { name: '绑定项目', exact: true }).press('Control+Enter');
    await row.waitFor({ state: 'detached' });
  });

  await test('a failed autosave stays visible after exiting, retries and preserves local text through reload', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '原始需求')] });
    const row = page.locator('[data-inbox-id="a"]');
    await row.locator('.inbox-edit-target').dblclick();
    state.fail = true;
    await fillInboxEditor(row.getByRole('textbox'), '离开编辑前最后一次输入');
    await row.getByRole('textbox').press('Meta+Enter');
    await row.getByRole('alert').filter({ hasText: '暂时无法保存' }).waitFor();
    assert.equal(await row.getByRole('textbox').count(), 0);
    assert.equal(await row.locator('.inbox-markdown').innerText(), '离开编辑前最后一次输入');
    assert.equal(state.items[0].markdown, '原始需求');
    state.fail = true;
    await page.reload();
    assert.equal(await inboxEditorValue(row.getByRole('textbox')), '离开编辑前最后一次输入');
    await row.getByRole('alert').filter({ hasText: '暂时无法保存' }).waitFor();
    await row.getByRole('button', { name: '重试', exact: true }).click();
    await eventually(() => state.items[0].markdown === '离开编辑前最后一次输入');
    await row.getByRole('textbox').press('Escape');
    await row.getByRole('button', { name: '删除需求', exact: true }).waitFor();
  });

  await test('project binding, multi-label OR filters, configurable labels and agent execution use the same row', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '# 修复登录'), item('b', '更新文档', 'project-1', ['review'])] });
    const row = page.getByRole('article', { name: '修复登录' });
    await chooseProject(page, row.getByRole('button', { name: '绑定项目', exact: true }), 'AoW');
    await row.getByRole('button', { name: '选择标签', exact: true }).click();
    await page.getByRole('group', { name: '需求标签', exact: true }).getByRole('checkbox', { name: 'TODO', exact: true }).check();
    await page.getByRole('group', { name: '需求标签', exact: true }).getByRole('checkbox', { name: 'Review', exact: true }).check();
    await page.getByRole('button', { name: '按标签筛选', exact: true }).click();
    const filter = page.getByRole('dialog', { name: '按标签筛选（匹配任一标签）', exact: true });
    await filter.getByRole('checkbox', { name: 'TODO', exact: true }).check();
    assert.equal(await page.getByRole('article').count(), 1);
    await filter.getByRole('checkbox', { name: 'Review', exact: true }).check();
    assert.equal(await page.getByRole('article').count(), 2);
    await page.getByRole('button', { name: '按标签筛选', exact: true }).click();
    await chooseProject(page, page.getByRole('button', { name: '按项目筛选', exact: true }), 'AoW');
    assert.equal(await page.getByRole('article').count(), 1);
    await row.getByRole('button', { name: '执行需求', exact: true }).click();
    const executionPanel = page.getByRole('dialog', { name: '执行需求', exact: true });
    await executionPanel.getByRole('button', { name: '开始执行', exact: true }).click();
    await row.getByRole('button', { name: '打开终端', exact: true }).click();
    assert.equal(await page.locator('output').getAttribute('data-opened-terminal'), 'terminal-one');
    assert.equal(state.executions[0].markdown, '# 修复登录');
    await page.getByRole('button', { name: '配置标签', exact: true }).click();
    await page.getByRole('button', { name: '添加标签', exact: true }).click();
    await page.getByLabel('标签 5 名称', { exact: true }).fill('Waiting');
    await page.getByRole('button', { name: '保存标签', exact: true }).click();
    await page.getByRole('button', { name: '按标签筛选', exact: true }).click();
    await filter.getByRole('checkbox', { name: 'Waiting', exact: true }).waitFor();
    assert.equal(state.labels.at(-1).name, 'Waiting');
  });

  await test('toolbar follows panel width, keeps controls visible and bounds scrolling label menus', async t => {
    const { page } = await fixture(t, { width: 1200, items: [item('a', '需求内容')], labelList: [...labels, ...Array.from({ length: 36 }, (_, index) => ({ id: `label-${index}`, name: `用于检查窄面板的较长标签名称 ${index}`, color: '#94a3b8' }))] });
    const project = page.getByLabel('按项目筛选', { exact: true });
    const search = page.locator('.inbox-search');
    const filter = page.getByRole('button', { name: '按标签筛选', exact: true });
    const manage = page.getByRole('button', { name: '配置标签', exact: true });
    assert.equal(await page.locator('.inbox-heading').count(), 0);
    const wide = await Promise.all([project, search, filter, manage].map(control => control.boundingBox()));
    assert.ok(wide.every(bounds => Math.abs(bounds.y - wide[0].y) < 2));
    assert.ok(wide[0].x + wide[0].width < wide[1].x);
    assert.ok((await page.locator('.inbox-header').boundingBox()).height < 60);
    // Keep the browser wide: the Inbox pane itself is what gets smaller.
    await page.locator('#root').evaluate(element => { element.style.width = '420px'; });
    assert.ok((await search.boundingBox()).y < (await project.boundingBox()).y);
    assert.ok(Math.abs((await filter.boundingBox()).y - (await project.boundingBox()).y) < 2);
    assert.ok((await page.locator('.inbox-header').boundingBox()).height < 100);
    await page.locator('#root').evaluate(element => { element.style.width = '260px'; element.style.height = '360px'; });
    assert.equal(await page.locator('.inbox-filter-text').isVisible(), false);
    const pane = await page.locator('.inbox-panel').boundingBox();
    for (const control of [project, search, filter, manage]) {
      const bounds = await control.boundingBox();
      assert.ok(bounds.x >= pane.x && bounds.x + bounds.width <= pane.x + pane.width + 1);
    }
    await project.press('ArrowDown');
    const projects = page.getByRole('listbox', { name: '按项目筛选', exact: true });
    const projectBounds = await projects.boundingBox();
    assert.ok(projectBounds.x >= pane.x && projectBounds.x + projectBounds.width <= pane.x + pane.width);
    assert.equal(await projects.getByRole('option', { name: '所有项目', exact: true }).evaluate(element => element === document.activeElement), true);
    await page.keyboard.press('End');
    await page.keyboard.press('Enter');
    assert.equal(await project.innerText(), 'Website');
    assert.equal(await projects.count(), 0);
    assert.equal(await project.evaluate(element => element === document.activeElement), true);
    await project.press('ArrowDown');
    assert.equal(await projects.getByRole('option', { selected: true }).count(), 1);
    assert.equal(await projects.getByRole('option', { name: 'Website', exact: true }).evaluate(element => element === document.activeElement), true);
    await page.keyboard.press('Home');
    await page.keyboard.press('Enter');
    assert.equal(await project.innerText(), '所有项目');
    const before = await page.getByRole('article').boundingBox();
    await manage.click();
    const menu = page.getByRole('dialog', { name: '配置标签', exact: true });
    const bounds = await menu.boundingBox();
    assert.ok(bounds.x >= pane.x && bounds.x + bounds.width <= pane.x + pane.width + 1);
    assert.ok(bounds.y + bounds.height <= pane.y + pane.height + 1);
    assert.equal((await page.getByRole('article').boundingBox()).y, before.y);
    const labelRows = menu.locator('.inbox-label-editor-list');
    assert.equal(await labelRows.evaluate(element => element.scrollHeight > element.clientHeight), true);
    await labelRows.evaluate(element => { element.scrollTop = element.scrollHeight; });
    const save = await menu.getByRole('button', { name: '保存标签', exact: true }).boundingBox();
    const last = await menu.getByRole('textbox', { name: '标签 40 名称', exact: true }).boundingBox();
    assert.ok(save.y < pane.y + pane.height);
    assert.ok(last.y + last.height <= save.y);
    const remove = await menu.getByRole('button', { name: '删除标签 用于检查窄面板的较长标签名称 35', exact: true }).boundingBox();
    assert.ok(remove.x + remove.width <= bounds.x + bounds.width);
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-narrow-label-menu.png` });
    await page.keyboard.press('Escape');
    assert.equal(await menu.count(), 0);
    assert.equal(await manage.evaluate(element => element === document.activeElement), true);
    await filter.click();
    const picker = page.getByRole('dialog', { name: '按标签筛选（匹配任一标签）', exact: true });
    const filterBounds = await picker.boundingBox();
    assert.ok(filterBounds.x >= pane.x && filterBounds.x + filterBounds.width <= pane.x + pane.width);
    assert.ok(filterBounds.y + filterBounds.height <= pane.y + pane.height);
    assert.equal(await picker.locator('.inbox-filter-list').evaluate(element => element.scrollHeight > element.clientHeight), true);
    await picker.getByRole('checkbox', { name: 'TODO', exact: true }).check();
    await picker.getByRole('checkbox', { name: 'Review', exact: true }).check();
    assert.equal(await filter.innerText(), '2');
    await page.screenshot({ path: `${screenshots}/inbox-narrow-filter-menu.png` });
    await page.getByRole('button', { name: '添加需求', exact: true }).click();
    assert.equal(await picker.count(), 0);
  });

  await test('label menu preserves edits after a failed save and removes labels from existing requirements', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '待处理需求', null, ['todo'])] });
    await page.getByRole('button', { name: '配置标签', exact: true }).click();
    const menu = page.getByRole('dialog', { name: '配置标签', exact: true });
    await menu.getByRole('button', { name: '删除标签 TODO', exact: true }).click();
    await menu.getByText('保存后，已删除的标签也会从需求上移除。', { exact: true }).waitFor();
    state.fail = true;
    await menu.getByRole('button', { name: '保存标签', exact: true }).click();
    await menu.getByRole('alert').filter({ hasText: '暂时无法保存' }).waitFor();
    assert.equal(await menu.getByRole('button', { name: '删除标签 TODO', exact: true }).count(), 0);
    assert.equal(state.items[0].label_ids.length, 1);
    await menu.getByRole('button', { name: '保存标签', exact: true }).click();
    await menu.waitFor({ state: 'detached' });
    assert.equal(state.labels.some(label => label.id === 'todo'), false);
    assert.deepEqual(state.items[0].label_ids, []);
    await page.getByRole('button', { name: '配置标签', exact: true }).click();
    await page.getByRole('textbox', { name: '搜索需求', exact: true }).click();
    assert.equal(await menu.count(), 0);
  });

  await test('drag and keyboard reorder preserve hidden rows and persist through refresh', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '第一项', 'project-0'), item('b', '其他项目', 'project-1'), item('c', '最后一项', 'project-0')] });
    await chooseProject(page, page.getByRole('button', { name: '按项目筛选', exact: true }), 'AoW');
    const handle = page.getByRole('button', { name: '拖动排序：最后一项' });
    const start = await handle.boundingBox(), target = await page.getByRole('article', { name: '第一项' }).boundingBox();
    await page.mouse.move(start.x + start.width / 2, start.y + start.height / 2); await page.mouse.down();
    await page.mouse.move(target.x + 50, target.y + 5, { steps: 12 }); await page.mouse.up();
    await page.waitForFunction(() => document.querySelector('.inbox-row').dataset.inboxId === 'c');
    assert.deepEqual(state.items.map(item => item.id), ['c', 'a', 'b']);
    await page.reload();
    await page.getByRole('button', { name: '拖动排序：最后一项' }).press('ArrowDown');
    await page.waitForFunction(() => document.querySelector('.inbox-row').dataset.inboxId === 'a');
    assert.deepEqual(state.items.map(item => item.id), ['a', 'c', 'b']);
  });

  await test('remote edits do not overwrite an open draft; conflict recovery is explicit and deletion is inline', async t => {
    const { page, state } = await fixture(t, { items: [item('a', '原始内容')] });
    const row = page.locator('[data-inbox-id="a"]');
    await row.locator('.inbox-edit-target').dblclick();
    await fillInboxEditor(row.getByRole('textbox', { name: '需求 Markdown', exact: true }), '本地草稿');
    await chooseProject(page, row.getByRole('button', { name: '绑定项目', exact: true }), 'AoW');
    await row.getByRole('button', { name: '选择标签', exact: true }).click();
    await page.getByRole('dialog', { name: '选择需求标签', exact: true }).getByRole('checkbox', { name: 'TODO', exact: true }).check();
    await page.keyboard.press('Escape');
    Object.assign(state.items[0], { markdown: '远端更新', project_id: 'project-1', label_ids: ['review'] }); state.items[0].revision++; state.revision++;
    await page.evaluate(() => window.emitLiveEvent('workspace', { boot_id: 'watch-fixture', revision: 1, projects: 0, terminals: 0, inbox: 1, repositories: {} }));
    await row.getByRole('button', { name: '保留草稿，以最新版本保存' }).waitFor();
    assert.equal(await inboxEditorValue(row.getByRole('textbox', { name: '需求 Markdown', exact: true })), '本地草稿');
    assert.equal(await row.getByRole('button', { name: '绑定项目', exact: true }).innerText(), 'AoW');
    assert.equal(await row.getByRole('button', { name: '选择标签', exact: true }).innerText(), 'TODO');
    await row.getByRole('button', { name: '保留草稿，以最新版本保存' }).click();
    await row.getByRole('textbox').press('Control+Enter');
    await row.getByRole('button', { name: '删除需求', exact: true }).waitFor();
    assert.equal(state.items[0].project_id, 'project-0');
    assert.deepEqual(state.items[0].label_ids, ['todo']);
    await row.getByRole('button', { name: '删除需求', exact: true }).click();
    assert.equal(await page.getByRole('dialog').count(), 0);
    await row.getByRole('button', { name: '确认删除', exact: true }).click();
    await row.waitFor({ state: 'detached' }); assert.equal(state.items.length, 0);
  });

  await test('bottom capture stays visible in a long list and Markdown is sanitized at mobile widths', async t => {
    const { page } = await fixture(t, { width: 390, items: Array.from({ length: 25 }, (_, index) => item(String(index), `# 需求 ${index}\n\n一段可以随时补充的 Markdown 描述。${index === 0 ? '<script>window.inboxInjected=true</script><img src=x onerror="window.inboxInjected=true">' : ''}`)) });
    const button = page.getByRole('button', { name: '添加需求', exact: true });
    const before = await button.boundingBox();
    await page.locator('.inbox-scroll').evaluate(element => { element.scrollTop = element.scrollHeight; });
    const after = await button.boundingBox(); assert.equal(before.y, after.y); assert.ok(after.y + after.height <= 720);
    await button.click(); await fillInboxEditor(page.getByRole('textbox', { name: '新需求 Markdown' }), '手机快速录入');
    const input = await page.locator('.inbox-new-row .inbox-code-editor').boundingBox(); assert.ok(input.y >= 0 && input.y < after.y);
    assert.equal(await page.evaluate(() => window.inboxInjected), undefined);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), true);
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-mobile.png` });
  });

  await test('desktop renders rich requirements with compact actions and a fixed capture bar', async t => {
    const { page } = await fixture(t, { items: [item('a', '# 支持导出执行报告\n\n在会话详情中增加导出入口，支持 **Markdown** 格式。\n\n- [ ] 保留代码块和引用\n- [ ] 包含执行时间与 Agent 名称', 'project-0', ['todo']), item('b', '整理项目初始化流程\n\n减少首次打开项目时的必填项，让用户先开始工作。', 'project-0', ['progress']), item('c', '# 手机端快速记录\n\n随时记录一个想法，晚点再分配项目。', null, ['review'])] });
    const screenshots = process.env.AOW_TEST_SCREENSHOT_DIR || '/private/tmp/aow-inbox-screenshots'; await mkdir(screenshots, { recursive: true });
    await page.screenshot({ path: `${screenshots}/inbox-desktop.png` });
  });
} finally { await browser?.close(); await server.close(); }
