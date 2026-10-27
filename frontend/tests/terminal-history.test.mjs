import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { VtService } from '../../vt-worker/src/vt-service.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
const service = new VtService();
const identity = { session_id: 'history', generation: 'epoch' };
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  for (const compressed of [false, true]) await test(`browser reload restores full history with ${compressed ? 'compressed snapshots beyond the raw replay ring' : 'inline snapshots'}`, async t => {
    t.after(() => service.disposeAll());
    await service.dispatch('create', { ...identity, cols: compressed ? 220 : 80, rows: 24, initial_offset: 0 });
    const colored = compressed ? '\x1b[38;2;123;45;67ma\x1b[38;2;89;123;45mb'.repeat(95) : '\x1b[38;2;123;45;67m红\x1b[38;2;89;123;45m绿'.repeat(15);
    const data = Buffer.from(Array.from({ length: 5_000 }, (_, i) => `line-${String(i).padStart(5, '0')} ${colored}\x1b[0m`).join('\r\n'));
    for (let offset = 0; offset < data.length; offset += 64 * 1024) {
      await service.dispatch('write', { ...identity, start_offset: offset, data: data.subarray(offset, offset + 64 * 1024) });
    }
    let snapshot = await service.dispatch('snapshot', identity);
    assert.ok((snapshot.decoded_byte_length ?? snapshot.byte_length) > 2 * 1024 * 1024);
    if (compressed) {
      assert.ok(data.length > 16 * 1024 * 1024, 'the previous snapshot plus 8 MiB ring cannot recover this history');
      assert.equal(snapshot.encoding, 'gzip-base64');
      assert.ok(snapshot.decoded_byte_length > 8 * 1024 * 1024);
    }
    assert.ok(snapshot.byte_length <= 8 * 1024 * 1024);

    const page = await browser.newPage({ viewport: { width: compressed ? 4400 : 1600, height: 900 } });
    t.after(() => page.close());
    page.setDefaultTimeout(15_000);
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/api/**', route => route.fulfill({ status: 404, json: {} }));
    await page.routeWebSocket('**/api/terminals/**/ws?**', socket => {
      socket.onMessage(message => {
        if (typeof message !== 'string' || JSON.parse(message).type !== 'claim') return;
        socket.send(JSON.stringify({ type: 'control', state: 'claimed' }));
        socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch', reset: true,
          offset: snapshot.applied_offset, replay_bytes: 0, restore_cols: snapshot.cols,
          restore_rows: snapshot.rows, ...(snapshot.encoding === 'gzip-base64'
              ? { restore_gzip: { data: snapshot.data.toString('utf8'), decoded_bytes: snapshot.decoded_byte_length } }
              : { restore: snapshot.data.toString('utf8') }) }));
      });
    });
    const url = `http://127.0.0.1:${server.httpServer.address().port}/tests/terminal-session-preview.html?history`;
    await page.goto(url);
    const readHistory = async () => {
      await page.waitForFunction(() => window.terminalHistoryPreview?.length === 2
        && window.terminalHistoryPreview.every(terminal => terminal.buffer.normal.length >= 5_000)
        && !document.querySelector('.restore-pending')).catch(async error => {
          t.diagnostic(JSON.stringify(await page.evaluate(() => ({ text: document.body.innerText.slice(-500),
            terminals: window.terminalHistoryPreview?.map(terminal => ({ cols: terminal.cols, rows: terminal.rows, lines: terminal.buffer.normal.length })) }))));
          throw error;
        });
      return page.evaluate(() => window.terminalHistoryPreview.map(terminal => ({
        scrollback: terminal.options.scrollback,
        active: terminal.buffer.active.type,
        lines: Array.from({ length: terminal.buffer.normal.length }, (_, i) =>
          terminal.buffer.normal.getLine(i).translateToString(true)),
      })));
    };
    const initial = await readHistory();
    if (compressed) assert.equal(await page.evaluate(() => window.terminalHistoryPreview[0].buffer.normal.getLine(0).getCell(11).getFgColor()), 0x7b2d43);
    for (const terminal of initial) {
      assert.equal(terminal.scrollback, 5_000);
      assert.equal(terminal.lines.length, 5_000);
      assert.equal(terminal.lines[0], `line-00000 ${compressed ? 'ab'.repeat(95) : '红绿'.repeat(15)}`);
      assert.equal(terminal.lines.at(-1), `line-04999 ${compressed ? 'ab'.repeat(95) : '红绿'.repeat(15)}`);
    }
    await page.reload();
    assert.deepEqual(await readHistory(), initial);

    // Continue parsing while the browser is absent, then restore a fullscreen
    // app with its normal history still available behind the alternate screen.
    await page.goto('about:blank');
    const suffix = Buffer.from('\r\noffline-output\x1b[?1049h\x1b[2J\x1b[Hfullscreen');
    await service.dispatch('write', { ...identity, start_offset: data.length, data: suffix });
    snapshot = await service.dispatch('snapshot', identity);
    await page.goto(url);
    for (const terminal of await readHistory()) {
      assert.equal(terminal.active, 'alternate');
      assert.deepEqual(terminal.lines.slice(0, 5_000), initial[0].lines);
      assert.equal(terminal.lines.at(-1), 'offline-output');
    }
    assert.deepEqual(errors, []);
  });
} finally {
  service.disposeAll();
  await browser?.close();
  await server.close();
}
