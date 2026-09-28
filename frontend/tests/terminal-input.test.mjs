import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { createServer } from 'vite';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
const dragPaths = ['/workspace/demo', '/workspace/demo/目录 with spaces', "/workspace/demo/a'b $HOME;`whoami`.txt"];

try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const url = `http://127.0.0.1:${server.httpServer.address().port}/tests/terminal-session-preview.html`;

  async function fixture(t, bracketed = true, explorer = false) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.setDefaultTimeout(15_000);
    t.after(() => page.close());
    const errors = [], inputs = [], resizes = [], sockets = new Map(), observers = new Set();
    page.on('pageerror', error => errors.push(error.stack ?? error.message));
    t.after(() => assert.deepEqual(errors, []));
    await page.route('**/api/**', route => {
      const path = new URL(route.request().url()).pathname;
      if (path.startsWith('/api/fs/tree/')) return route.fulfill({ json: { path: dragPaths[0], entries: dragPaths.slice(1).map((path, index) => ({
        path, name: path.split('/').pop(), kind: index === 0 ? 'directory' : 'file', size: 1, hidden: false, readonly: false,
      })) } });
      if (path === '/api/git/ignored') return route.fulfill({ json: { ignored: [] } });
      return route.fulfill({ status: 404, json: {} });
    });
    await page.routeWebSocket('**/api/terminals/**/ws?**', socket => {
      const id = new URL(socket.url()).pathname.split('/').at(-2);
      sockets.set(id, socket);
      socket.onMessage(message => {
        if (typeof message !== 'string') { inputs.push({ id, text: message.toString() }); return; }
        const control = JSON.parse(message);
        if (control.type === 'resize') resizes.push({ id, ...control });
        if (control.type === 'claim') {
          if (control.force) observers.delete(id);
          socket.send(JSON.stringify({ type: 'control', state: observers.has(id) ? 'observing' : 'claimed' }));
          socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch', offset: 0, reset: true, replay_bytes: 0,
            restore_cols: 80, restore_rows: 24,
            restore: `\x1b[2J\x1b[HKEEP TERMINAL ALIVE\r\n$ ${bracketed ? '\x1b[?2004h' : ''}` }));
        }
      });
    });
    await page.goto(`${url}${explorer ? '?explorer' : ''}`);
    await page.locator('.terminal-input-trigger').nth(1).waitFor();
    await delay(250);
    const pane = page.locator('.terminal-pane').first();
    return { page, pane, inputs, resizes, sockets, observers };
  }

  async function open(pane) {
    await pane.getByRole('button', { name: '打开终端输入编辑器' }).click();
    const editor = pane.getByRole('textbox', { name: '终端 Markdown 输入' });
    await editor.waitFor();
    await editor.focus();
    return editor;
  }

  async function value(page) {
    return page.evaluate(async () => {
      const { monaco } = await import('/src/features/editor/monaco.ts');
      return monaco.editor.getModels().find(model => model.getLanguageId() === 'markdown')?.getValue();
    });
  }

  async function dropData(page, target, type, path) {
    const dataTransfer = await page.evaluateHandle(({ type, path }) => {
      const data = new DataTransfer(); data.setData(type, path); return data;
    }, { type, path });
    await target.dispatchEvent('dragover', { dataTransfer });
    await target.dispatchEvent('drop', { dataTransfer });
    await dataTransfer.dispose();
  }

  await test('Explorer root, directories and files drag into the target split as quoted paste without Enter', async t => {
    const { page, pane, inputs } = await fixture(t, true, true);
    const target = page.locator('.terminal-pane').nth(1);
    await pane.locator('.xterm-helper-textarea').focus();
    for (const path of dragPaths) {
      const source = page.locator('.tree-row').filter({ has: page.locator('.tree-label', { hasText: path.split('/').pop() }) });
      await source.dragTo(target.locator('.terminal-emulator'));
    }
    await delay(100);
    assert.deepEqual(inputs, [
      { id: 'two', text: "\x1b[200~'/workspace/demo' \x1b[201~" },
      { id: 'two', text: "\x1b[200~'/workspace/demo/目录 with spaces' \x1b[201~" },
      { id: 'two', text: "\x1b[200~'/workspace/demo/a'\\''b $HOME;`whoami`.txt' \x1b[201~" },
    ]);
    assert.equal(await target.locator('.xterm-helper-textarea').evaluate(node => node === document.activeElement), true);
    assert.equal(await page.locator('.terminal-path-drop-overlay').count(), 0);
    assert.equal(await page.locator('.pane-drop-target').count(), 0);
    assert.equal(await page.locator('.terminal-pane').count(), 2);
  });

  await test('path drag feedback clears on leave and loss of control rejects a subsequent drop', async t => {
    const { page, pane, inputs, sockets, observers } = await fixture(t);
    const target = pane.locator('.terminal-emulator-shell');
    const dataTransfer = await page.evaluateHandle(() => {
      const data = new DataTransfer(); data.setData('application/x-aow-explorer-path', '/workspace/demo'); return data;
    });
    await target.dispatchEvent('dragover', { dataTransfer });
    await pane.getByText('松开以输入路径').waitFor();
    await target.dispatchEvent('dragleave', { dataTransfer });
    assert.equal(await pane.locator('.terminal-path-drop-overlay').count(), 0);
    await target.dispatchEvent('dragover', { dataTransfer });
    observers.add('one');
    sockets.get('one').send(JSON.stringify({ type: 'error', code: 'attachment_superseded' }));
    await pane.getByRole('button', { name: '接管', exact: true }).waitFor();
    await pane.locator('.terminal-path-drop-overlay').waitFor({ state: 'detached' });
    await target.dispatchEvent('drop', { dataTransfer });
    await dataTransfer.dispose();
    await delay(100);
    assert.deepEqual(inputs, []);
  });

  await test('unrelated drags, control characters and suspended terminal input never send paths', async t => {
    const { page, pane, inputs } = await fixture(t);
    const target = pane.locator('.terminal-emulator-shell');
    for (const type of ['text/plain', 'application/x-aow-terminal-pane', 'application/x-aow-pinned-worktree']) {
      await dropData(page, target, type, '/workspace/demo');
    }
    for (const path of ['', 'relative.txt', '/tmp/line\nbreak', '/tmp/carriage\rreturn', '/tmp/escape\x1b[201~', '/tmp/tab\t', '/tmp/nul\0']) {
      await dropData(page, target, 'application/x-aow-explorer-path', path);
    }
    await open(pane);
    await dropData(page, target, 'application/x-aow-explorer-path', '/workspace/demo');
    await delay(100);
    assert.deepEqual(inputs, []);
    assert.equal(await pane.locator('.terminal-path-drop-overlay').count(), 0);
  });

  await test('path drops without bracketed paste still never submit', async t => {
    const { page, pane, inputs } = await fixture(t, false, true);
    await page.locator('.workspace-root-node').dragTo(pane.locator('.terminal-emulator'));
    await delay(100);
    assert.deepEqual(inputs, [{ id: 'one', text: "'/workspace/demo' " }]);
  });

  await test('hover entry, push-up and height dragging keep terminal geometry and PTY unchanged; Shift+Enter submits once', async t => {
    const { page, pane, inputs, resizes } = await fixture(t);
    const trigger = pane.getByRole('button', { name: '打开终端输入编辑器' });
    await page.mouse.move(10, 10);
    assert.equal(await trigger.locator('span').evaluate(node => getComputedStyle(node).opacity), '0');
    assert.ok((await trigger.boundingBox()).height <= 9);
    await trigger.hover();
    assert.equal(await trigger.locator('span').evaluate(node => getComputedStyle(node).opacity), '1');
    const before = await pane.locator('.terminal-emulator').boundingBox();
    await pane.locator('.xterm').evaluate(node => { window.originalXterm = node; });
    const resizeCount = resizes.length;
    const editor = await open(pane);
    await page.keyboard.insertText('# 计划\n\n- 修改输入交互\n- 保持终端尺寸');
    await delay(200);
    const after = await pane.locator('.terminal-emulator').boundingBox();
    const composer = await pane.locator('.terminal-input-composer').boundingBox();
    assert.equal(after.width, before.width);
    assert.equal(after.height, before.height);
    assert.ok(Math.abs(before.y - after.y - composer.height) < 1);
    assert.ok(Math.abs(after.y + after.height - composer.y) < 1, 'terminal ends where the editor starts');
    const grip = await pane.getByRole('separator', { name: '调整输入编辑器高度' }).boundingBox();
    await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
    await page.mouse.down();
    await page.mouse.move(grip.x + grip.width / 2, grip.y - 90, { steps: 6 });
    await page.mouse.up();
    assert.ok((await pane.locator('.terminal-input-composer').boundingBox()).height > composer.height + 80);
    assert.equal(await editor.evaluate(node => node === document.activeElement), true, 'drag retains editor focus');
    await page.screenshot({ path: `/tmp/aow-terminal-input-${process.pid}.png` });
    await pane.locator('.xterm-helper-textarea').evaluate(node => {
      const clipboardData = new DataTransfer();
      clipboardData.setData('text/plain', 'must not reach the agent');
      node.dispatchEvent(new ClipboardEvent('paste', { clipboardData, bubbles: true, cancelable: true }));
    });
    assert.deepEqual(inputs, [], 'editing must not send terminal input');
    await editor.press('Shift+Enter');
    await pane.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    await delay(250);
    assert.deepEqual(inputs, [{ id: 'one', text: '\x1b[200~# 计划\r\r- 修改输入交互\r- 保持终端尺寸\x1b[201~\r' }]);
    assert.equal(resizes.length, resizeCount, 'opening, dragging and closing never resize the PTY');
    assert.deepEqual(await pane.locator('.terminal-emulator').boundingBox(), before);
    assert.equal(await pane.locator('.xterm').evaluate(node => node === window.originalXterm), true);
    assert.equal(await pane.locator('.xterm-helper-textarea').evaluate(node => node === document.activeElement), true);
    await open(pane);
    assert.equal(await value(page), '', 'successful submit clears the draft');
  });

  await test('blur and outside clicks immediately dismiss, preserve per-pane drafts and do not steal focus', async t => {
    const { page, pane, inputs } = await fixture(t);
    await open(pane);
    await page.keyboard.insertText('第一份草稿');
    const other = page.locator('.terminal-pane').nth(1);
    await other.locator('.xterm-helper-textarea').focus();
    assert.equal(await pane.locator('.terminal-input-composer').count(), 0);
    assert.equal(await other.locator('.xterm-helper-textarea').evaluate(node => node === document.activeElement), true);
    await open(other);
    assert.equal(await value(page), '');
    await page.keyboard.insertText('第二份草稿');
    await page.mouse.click(5, 5);
    await other.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    await open(pane);
    assert.equal(await value(page), '第一份草稿');
    await page.evaluate(() => window.dispatchEvent(new Event('blur')));
    await pane.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    await open(other);
    assert.equal(await value(page), '第二份草稿');
    await other.getByRole('textbox', { name: '终端 Markdown 输入' }).press('Escape');
    await other.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    await open(other);
    assert.equal(await value(page), '第二份草稿', 'Escape dismisses without submitting');
    await other.locator('.terminal-emulator').click({ position: { x: 50, y: 500 } });
    await other.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    assert.equal(await other.locator('.xterm-helper-textarea').evaluate(node => node === document.activeElement), true);
    assert.deepEqual(inputs, []);
  });

  await test('Enter inserts a newline, Shift+Enter submits, and IME confirmation never submits', async t => {
    const { page, pane, inputs } = await fixture(t);
    const editor = await open(pane);
    await editor.press('Shift+Enter');
    assert.equal(await pane.locator('.terminal-input-composer').count(), 1, 'empty drafts do not submit');
    await page.keyboard.insertText('第一行');
    await editor.press('Enter');
    await page.keyboard.insertText('第二行');
    assert.equal(await value(page), '第一行\n第二行');
    await editor.evaluate(node => {
      node.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
      node.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', shiftKey: true, keyCode: 229, isComposing: true, bubbles: true, cancelable: true }));
      node.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true, data: '' }));
      node.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, keyCode: 229, bubbles: true, cancelable: true }));
    });
    assert.deepEqual(inputs, []);
    assert.equal(await pane.locator('.terminal-input-composer').count(), 1);
    await editor.press('Shift+Enter');
    await pane.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    assert.deepEqual(inputs, [{ id: 'one', text: '\x1b[200~第一行\r第二行\x1b[201~\r' }]);
  });

  await test('loss of terminal control closes the editor without sending or discarding the draft', async t => {
    const { page, pane, inputs, sockets, observers } = await fixture(t);
    await open(pane);
    await page.keyboard.insertText('保留未提交内容');
    observers.add('one');
    sockets.get('one').send(JSON.stringify({ type: 'error', code: 'attachment_superseded' }));
    await pane.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    assert.equal(await pane.locator('.terminal-input-trigger').count(), 0);
    assert.deepEqual(inputs, []);
    await pane.getByRole('button', { name: '接管', exact: true }).click();
    await open(pane);
    assert.equal(await value(page), '保留未提交内容');
  });

  await test('terminals without bracketed paste use ordinary paste and one Enter', async t => {
    const { page, pane, inputs } = await fixture(t, false);
    const editor = await open(pane);
    await page.keyboard.insertText('echo hello');
    await editor.press('Shift+Enter');
    await pane.locator('.terminal-input-composer').waitFor({ state: 'detached' });
    assert.deepEqual(inputs, [{ id: 'one', text: 'echo hello\r' }]);
  });
} finally {
  await browser?.close();
  await server.close();
}
