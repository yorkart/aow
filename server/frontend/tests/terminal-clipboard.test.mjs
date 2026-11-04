import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { createServer } from 'vite';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
const osc52 = (text, selection = 'c') => `\x1b]52;${selection};${Buffer.from(text).toString('base64')}\x07`;
const fullscreen = '\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[2J\x1b[HFull-screen application selection';
let browser;

try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;

  async function fixture(t, { insecure = false, observing = false, replay = '' } = {}) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    // Grant readback only on localhost; the HTTP terminal must copy using the
    // actual drag gesture, without an insecure-origin permission override.
    await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base });
    const page = await context.newPage();
    page.setDefaultTimeout(10_000);
    page.setDefaultNavigationTimeout(30_000);
    const errors = [], inputs = [], sockets = new Map();
    let selectionText;
    let marker = 0;
    let observer = observing;
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await context.close(); assert.deepEqual(errors, []); });
    await page.addInitScript(() => {
      localStorage.setItem('aow.language', 'zh-CN');
      window.clipboardWrites = [];
      window.copyFallbacks = [];
      if (navigator.clipboard) {
        const write = navigator.clipboard.writeText.bind(navigator.clipboard);
        navigator.clipboard.writeText = text => { window.clipboardWrites.push(text); return write(text); };
      }
      const copy = document.execCommand.bind(document);
      document.execCommand = (...args) => {
        if (args[0] === 'copy') window.copyFallbacks.push(document.activeElement?.value);
        return copy(...args);
      };
    });
    const origin = insecure ? `http://terminal.test:${server.httpServer.address().port}` : base;
    if (insecure) await page.route(`${origin}/**`, async route => {
      const url = new URL(route.request().url());
      const response = await route.fetch({ url: `${base}${url.pathname}${url.search}`,
        headers: { ...route.request().headers(), host: new URL(base).host } });
      await route.fulfill({ response });
    });
    await page.route('**/api/**', route => route.fulfill({ status: 404, json: {} }));
    await page.routeWebSocket('**/api/terminals/**/ws?**', socket => {
      const id = new URL(socket.url()).pathname.split('/').at(-2);
      sockets.set(id, socket);
      socket.onClose(() => socket.close());
      socket.onMessage(message => {
        if (typeof message !== 'string') {
          inputs.push(message.toString());
          // Simulate a TUI that copies its own selection on button release.
          // The copy bytes travel back through the socket and xterm parser.
          // eslint-disable-next-line no-control-regex -- Match the SGR mouse release protocol.
          if (id === 'one' && selectionText && /\x1b\[<0;\d+;\d+m/.test(message.toString())) {
            socket.send(Buffer.from(osc52(selectionText)));
            selectionText = undefined;
          }
          return;
        }
        if (JSON.parse(message).type !== 'claim') return;
        socket.send(JSON.stringify({ type: 'control', state: observer ? 'observing' : 'claimed' }));
        socket.send(JSON.stringify({ type: 'stream', epoch: 'clipboard', offset: 0, reset: true, replay_bytes: 0,
          restore_cols: 80, restore_rows: 24, restore: fullscreen + replay }));
      });
    });
    await page.goto(`${origin}/tests/terminal-session-preview.html?history`);
    await page.waitForFunction(() => window.terminalHistoryPreview?.length === 2
      && !document.querySelector('.restore-pending'));
    await page.evaluate(() => {
      window.clipboardMarkers = [];
      window.terminalHistoryPreview.forEach((terminal, index) => {
        terminal.onTitleChange(title => { window.clipboardMarkers[index] = title; });
      });
    });
    const pane = page.locator('.terminal-pane').first();
    await pane.locator('.xterm-helper-textarea').focus();
    return {
      page, pane, inputs,
      async send(data, id = 'one') {
        const title = `clipboard-${++marker}`;
        sockets.get(id).send(Buffer.from(`${data}\x1b]2;${title}\x07`));
        await page.waitForFunction(({ index, title }) => window.clipboardMarkers[index] === title,
          { index: id === 'one' ? 0 : 1, title });
      },
      async select(text) {
        selectionText = text;
        const screen = await pane.locator('.xterm-screen').boundingBox();
        await page.mouse.move(screen.x + 5, screen.y + 8);
        await page.mouse.down();
        await page.mouse.move(screen.x + 160, screen.y + 8, { steps: 4 });
        await page.mouse.up();
      },
      async readClipboard() {
        if (!insecure) return page.evaluate(() => navigator.clipboard.readText());
        const reader = await context.newPage();
        await reader.route('**/*', route => route.fulfill({ contentType: 'text/html', body: '<title>Clipboard readback</title>' }));
        await reader.goto(`${base}/clipboard-readback`);
        const text = await reader.evaluate(() => navigator.clipboard.readText());
        await reader.close();
        await page.bringToFront();
        return text;
      },
      async supersede() {
        observer = true;
        sockets.get('one').send(JSON.stringify({ type: 'error', code: 'attachment_superseded' }));
        await pane.getByRole('button', { name: '接管', exact: true }).waitFor();
      },
    };
  }

  async function copied(f, text) {
    await f.pane.locator('.terminal-clipboard-copy-status.success').waitFor();
    assert.equal(await f.readClipboard(), text);
    assert.equal(await f.page.locator('.terminal-clipboard-copy-status.pending').count(), 0);
    assert.equal(await f.pane.locator('.xterm-helper-textarea').evaluate(node => node === document.activeElement), true);
    assert.equal(await f.page.locator('body > textarea').count(), 0);
  }

  await test('full-screen mouse selection copies OSC 52 directly to the browser clipboard', async t => {
    const f = await fixture(t);
    const text = 'Codex selection\n中文 🦀';
    await f.select(text);
    await copied(f, text);
    assert.equal(await f.page.evaluate(() => window.terminalHistoryPreview[0].getSelection()), '',
      'the full-screen application owns the selection, not xterm');
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), []);
    assert.ok(f.inputs.some(input => input.endsWith('m')), 'the application received mouse release');
  });

  await test('LAN HTTP full-screen selection copies without another click or the Async Clipboard API', async t => {
    const f = await fixture(t, { insecure: true });
    assert.equal(await f.page.evaluate(() => window.isSecureContext), false);
    assert.equal(await f.page.evaluate(() => typeof navigator.clipboard), 'undefined');
    const text = 'HTTP selected text\n无需再次点击';
    await f.select(text);
    await copied(f, text);
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), [text]);
  });

  await test('a rejected Clipboard API write falls back automatically and preserves terminal focus', async t => {
    const f = await fixture(t);
    await f.page.evaluate(() => {
      navigator.clipboard.writeText = async () => { throw new DOMException('Denied', 'NotAllowedError'); };
    });
    const text = 'Fallback after browser rejection';
    await f.select(text);
    await copied(f, text);
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), [text]);
  });

  for (const loseControl of [false, true]) await test(`manual copy survives denied automatic writes${loseControl ? ' and loss of terminal control' : ''}`, async t => {
    const f = await fixture(t);
    await f.page.evaluate(() => {
      navigator.clipboard.writeText = async () => { throw new DOMException('Denied', 'NotAllowedError'); };
      const copy = document.execCommand.bind(document);
      document.execCommand = (...args) => window.allowManualCopy ? copy(...args) : false;
    });
    const text = 'Browser needs another gesture';
    await f.select(text);
    await f.pane.locator('.terminal-clipboard-copy-status.pending').waitFor();
    assert.equal(await f.pane.locator('.terminal-clipboard-copy-status.success').count(), 0);
    if (loseControl) await f.supersede();
    await f.page.evaluate(() => { window.allowManualCopy = true; });
    await f.pane.getByRole('button', { name: '复制', exact: true }).click();
    await copied(f, text);
  });

  await test('replay, inactive panes, observers and oversized requests cannot copy', async t => {
    const f = await fixture(t, { replay: osc52('historical copy') });
    await f.send(osc52('inactive pane'), 'two');
    await f.send(osc52('x'.repeat(256 * 1024 + 1)));
    await f.send(osc52('primary selection', 'p'));
    await f.supersede();
    await f.send(osc52('observer copy'));
    assert.deepEqual(await f.page.evaluate(() => window.clipboardWrites), []);
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), []);
    assert.equal(await f.page.locator('.terminal-clipboard-copy-status').count(), 0);
  });

  await test('a delayed rejection cannot copy over a newer request or steal focus from another pane', async t => {
    const f = await fixture(t);
    await f.page.evaluate(() => {
      const write = navigator.clipboard.writeText.bind(navigator.clipboard);
      navigator.clipboard.writeText = text => text === 'old request'
        ? new Promise((resolve, reject) => { window.rejectOldCopy = () => reject(new Error('Denied')); })
        : write(text);
    });
    await f.send(osc52('old request'));
    await f.select('new selection');
    await copied(f, 'new selection');
    await f.page.evaluate(() => window.rejectOldCopy());
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), []);
    assert.equal(await f.readClipboard(), 'new selection');
    await f.send(osc52('old request'));
    await f.page.locator('.terminal-pane').nth(1).locator('.xterm-helper-textarea').focus();
    await f.page.evaluate(() => window.rejectOldCopy());
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), []);
    assert.equal(await f.page.locator('.terminal-pane').nth(1).locator('.xterm-helper-textarea')
      .evaluate(node => node === document.activeElement), true);
  });

  await test('ordinary xterm selection still copies through the native copy event', async t => {
    const f = await fixture(t);
    await f.send('\x1b[?1002l\x1b[?1006l\x1b[?1049l\x1b[2J\x1b[Hnormal selection');
    const screen = await f.pane.locator('.xterm-screen').boundingBox();
    await f.page.mouse.move(screen.x + 1, screen.y + 8);
    await f.page.mouse.down();
    await f.page.mouse.move(screen.x + 150, screen.y + 8, { steps: 4 });
    await f.page.mouse.up();
    assert.equal(await f.page.evaluate(() => window.terminalHistoryPreview[0].getSelection()), 'normal selection');
    await f.page.keyboard.press(process.platform === 'darwin' ? 'Meta+c' : 'Control+Shift+c');
    assert.equal(await f.readClipboard(), 'normal selection');
    assert.deepEqual(await f.page.evaluate(() => window.copyFallbacks), []);
  });
} finally {
  await browser?.close();
  await server.close();
}
