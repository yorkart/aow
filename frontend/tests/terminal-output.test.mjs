import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { createServer } from 'vite';
import { VtService } from '../../vt-worker/src/vt-service.mjs';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: 0, proxy: {}, watch: { usePolling: true } } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'],
    ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const url = `http://127.0.0.1:${server.httpServer.address().port}/tests/terminal-session-preview.html?history`;

  async function fixture(t) {
    const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
    page.setDefaultTimeout(15_000);
    const service = new VtService();
    const identity = { session_id: 'output', generation: 'epoch' };
    const seed = Buffer.from('seed-history\r\n');
    const output = [seed];
    const connections = [];
    const inputs = [];
    const errors = [];
    let offset = seed.length;
    let snapshotsAvailable = true;
    let observing = false;
    await service.dispatch('create', { ...identity, cols: 80, rows: 24, initial_offset: 0 });
    await service.dispatch('write', { ...identity, start_offset: 0, data: seed });
    t.after(async () => { await page.close(); service.disposeAll(); assert.deepEqual(errors, []); });
    page.on('pageerror', error => errors.push(error.message));
    await page.route('**/api/**', route => route.fulfill({ status: 404, json: {} }));
    await page.routeWebSocket('**/api/terminals/**/ws?**', socket => {
      const parsed = new URL(socket.url());
      const id = parsed.pathname.split('/').at(-2);
      const connection = { socket, after: parsed.searchParams.get('after'), closed: false, ready: false };
      if (id === 'one') connections.push(connection);
      socket.onClose(() => { connection.closed = true; socket.close(); });
      socket.onMessage(async message => {
        if (typeof message !== 'string') { if (id === 'one') inputs.push(message.toString()); return; }
        if (JSON.parse(message).type !== 'claim') return;
        socket.send(JSON.stringify({ type: 'control', state: observing ? 'observing' : 'claimed' }));
        if (connection.after !== null) {
          const after = Number(connection.after);
          const suffix = Buffer.concat(output).subarray(after);
          socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch', offset: after, reset: false, replay_bytes: suffix.length }));
          for (let at = 0; at < suffix.length; at += 64 * 1024) socket.send(suffix.subarray(at, at + 64 * 1024));
        } else if (!snapshotsAvailable) {
          socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch', offset: 0, reset: true, replay_bytes: offset }));
          socket.send(Buffer.concat(output));
        } else {
          const snapshot = await service.dispatch('snapshot', identity);
          socket.send(JSON.stringify({ type: 'stream', epoch: 'epoch', offset: snapshot.applied_offset,
            reset: true, replay_bytes: 0, restore_cols: snapshot.cols, restore_rows: snapshot.rows,
            restore: snapshot.data.toString('utf8') }));
        }
        connection.ready = true;
      });
    });
    await page.goto(url);
    await page.waitForFunction(() => window.terminalHistoryPreview?.length === 2
      && !document.querySelector('.restore-pending'));
    await page.evaluate(() => {
      const terminal = window.terminalHistoryPreview[0];
      const write = terminal.write.bind(terminal), reset = terminal.reset.bind(terminal);
      const probe = { held: [], hold: true, bytes: 0, writes: 0, committed: 0, resets: 0, resetWhilePending: false };
      window.outputProbe = probe;
      terminal.write = (data, callback) => {
        probe.writes += 1;
        if (data instanceof Uint8Array) probe.bytes += data.length;
        const apply = () => write(data, () => { probe.committed += 1; callback?.(); });
        if (probe.hold) probe.held.push(apply);
        else apply();
      };
      terminal.reset = () => {
        probe.resets += 1;
        probe.resetWhilePending ||= probe.committed !== probe.writes;
        reset();
      };
      probe.release = () => { probe.hold = false; for (const apply of probe.held.splice(0)) apply(); };
    });
    return {
      page, connections, inputs,
      disableSnapshots() { snapshotsAvailable = false; },
      supersede() {
        observing = true;
        connections.at(-1).socket.send(JSON.stringify({ type: 'error', code: 'attachment_superseded' }));
      },
      offset: () => offset,
      async send(data, chunkSize = 64 * 1024, publish = true) {
        data = Buffer.from(data);
        await service.dispatch('write', { ...identity, start_offset: offset, data });
        offset += data.length;
        output.push(data);
        if (!publish) return;
        const socket = connections.at(-1).socket;
        for (let at = 0; at < data.length; at += chunkSize) socket.send(data.subarray(at, at + chunkSize));
      },
      async release() {
        await page.evaluate(() => window.outputProbe.release());
        await page.waitForFunction(() => window.outputProbe.committed === window.outputProbe.writes);
      },
      async text() {
        return page.evaluate(() => {
          const buffer = window.terminalHistoryPreview[0].buffer.normal;
          return Array.from({ length: buffer.length }, (_, i) => buffer.getLine(i).translateToString(true)).join('\n');
        });
      },
    };
  }

  async function connected(fixture, count) {
    for (let attempt = 0; attempt < 100; attempt += 1) {
      if (fixture.connections.length >= count && fixture.connections.at(-1).ready) {
        await fixture.page.waitForFunction(() => !document.querySelector('.restore-pending'));
        return;
      }
      await delay(50);
    }
    assert.fail('terminal did not reconnect');
  }

  await test('thousands of tiny split UTF-8 frames pipeline without resetting history', async t => {
    const f = await fixture(t);
    const data = Buffer.from(Array.from({ length: 600 }, (_, i) => `行-${String(i).padStart(4, '0')}\r\n`).join(''));
    await f.send(data, 3);
    await f.page.waitForFunction(bytes => window.outputProbe.bytes === bytes, data.length);
    const probe = await f.page.evaluate(() => ({ writes: window.outputProbe.writes, committed: window.outputProbe.committed }));
    assert.ok(probe.writes > 1, 'ordinary writes must be submitted before earlier callbacks');
    assert.equal(probe.committed, 0);
    assert.equal(f.connections.length, 1);
    assert.equal(f.connections[0].closed, false);
    await f.release();
    const text = await f.text();
    assert.match(text, /^seed-history\n行-0000\n/);
    for (let i = 0; i < 600; i += 1) assert.ok(text.includes(`行-${String(i).padStart(4, '0')}`));
    assert.equal(await f.page.evaluate(() => window.outputProbe.resets), 0);
  });

  await test('a 2 MiB burst and repeated drained bursts keep streaming on the same socket', async t => {
    const f = await fixture(t);
    const data = Buffer.concat([Buffer.alloc(2 * 1024 * 1024), Buffer.from('burst-0\r\n')]);
    await f.send(data, data.length);
    await f.page.waitForFunction(bytes => window.outputProbe.bytes === bytes, data.length);
    assert.equal(f.connections.length, 1);
    assert.equal(f.connections[0].closed, false);
    await f.release();
    for (let i = 1; i < 8; i += 1) {
      await f.send(Buffer.concat([Buffer.alloc(2 * 1024 * 1024), Buffer.from(`burst-${i}\r\n`)]));
      await f.page.waitForFunction(marker => {
        const buffer = window.terminalHistoryPreview[0].buffer.normal;
        return Array.from({ length: buffer.length }, (_, row) => buffer.getLine(row).translateToString(true)).includes(marker);
      }, `burst-${i}`);
    }
    assert.equal(f.connections.length, 1, 'total output volume is not a backlog signal');
    assert.equal(await f.page.evaluate(() => window.outputProbe.resets), 0);
    assert.match(await f.text(), /^seed-history\nburst-0\nburst-1\nburst-2/);
  });

  await test('network reconnect waits for every submitted write and resumes at the parsed offset', async t => {
    const f = await fixture(t);
    const data = Buffer.from('first\r\nsecond\r\n');
    await f.send(data, 7);
    await f.page.waitForFunction(bytes => window.outputProbe.bytes === bytes, data.length);
    f.connections[0].socket.close();
    await f.release();
    await connected(f, 2);
    assert.equal(Number(f.connections[1].after), f.offset());
    assert.match(await f.text(), /^seed-history\nfirst\nsecond\n/);
    assert.equal(await f.page.evaluate(() => window.outputProbe.resets), 0);
  });

  await test('replay completion keeps input disabled until the final parser callback', async t => {
    const f = await fixture(t);
    const data = Buffer.concat([Buffer.alloc(6 * 1024 * 1024), Buffer.from('replayed\r\n')]);
    await f.send(data, 64 * 1024, false);
    f.connections[0].socket.close();
    await f.page.waitForFunction(() => window.outputProbe.bytes === 4 * 1024 * 1024);
    await f.page.locator('.xterm-helper-textarea').first().focus();
    await f.page.keyboard.type('before');
    assert.deepEqual(f.inputs, []);
    await f.release();
    await f.page.waitForFunction(() => {
      const buffer = window.terminalHistoryPreview[0].buffer.normal;
      return Array.from({ length: buffer.length }, (_, i) => buffer.getLine(i).translateToString(true)).includes('replayed');
    });
    await f.page.keyboard.type('after');
    for (let attempt = 0; f.inputs.join('') !== 'after' && attempt < 100; attempt += 1) await delay(25);
    assert.equal(f.inputs.join(''), 'after');
    assert.match(await f.text(), /^seed-history\nreplayed\n/);
  });

  await test('takeover revokes input immediately and commits writes before observer resume', async t => {
    const f = await fixture(t);
    const data = Buffer.from('before-takeover\r\n');
    await f.send(data, 5);
    await f.page.waitForFunction(bytes => window.outputProbe.bytes === bytes, data.length);
    f.supersede();
    for (let attempt = 0; !f.connections[0].closed && attempt < 100; attempt += 1) await delay(25);
    assert.equal(f.connections[0].closed, true);
    await f.release();
    await connected(f, 2);
    assert.equal(Number(f.connections[1].after), f.offset());
    assert.match(await f.text(), /^seed-history\nbefore-takeover\n/);
    assert.equal(await f.page.evaluate(() => window.outputProbe.resets), 0);
    await f.page.locator('.xterm-helper-textarea').first().focus();
    await f.page.keyboard.type('not-a-controller');
    assert.deepEqual(f.inputs, []);
  });

  await test('a failed pipelined write recovers a snapshot instead of trusting a broken cursor', async t => {
    const f = await fixture(t);
    await f.release();
    await f.page.evaluate(() => {
      const terminal = window.terminalHistoryPreview[0], write = terminal.write.bind(terminal);
      let failed = false;
      terminal.write = (data, callback) => {
        if (!failed && data instanceof Uint8Array) { failed = true; throw new Error('parser fixture failure'); }
        write(data, callback);
      };
    });
    await f.send('first\r\nsecond\r\n', 7);
    await connected(f, 2);
    assert.equal(f.connections[1].after, null);
    assert.match(await f.text(), /^seed-history\nfirst\nsecond\n/);
    assert.equal(await f.page.evaluate(() => window.outputProbe.resets), 1);
  });

  await test('real parse backlog recovers a complete snapshot only after pending writes commit', async t => {
    const f = await fixture(t);
    const history = Array.from({ length: 600 }, (_, i) => `history-${String(i).padStart(4, '0')}\r\n`).join('');
    await f.send(Buffer.concat([Buffer.from(history), Buffer.alloc(5 * 1024 * 1024), Buffer.from('tail\r\n')]));
    for (let attempt = 0; !f.connections[0].closed && attempt < 100; attempt += 1) await delay(25);
    assert.equal(f.connections[0].closed, true, 'unparsed bytes inside xterm must count toward the bound');
    assert.equal(f.connections.length, 1, 'a reset must wait for pending parser callbacks');
    await f.release();
    await connected(f, 2);
    assert.equal(f.connections[1].after, null, 'overloaded consumer requests a full snapshot');
    const text = await f.text();
    assert.match(text, /^seed-history\nhistory-0000\n/);
    for (let i = 0; i < 600; i += 1) assert.ok(text.includes(`history-${String(i).padStart(4, '0')}`));
    assert.match(text, /history-0599\ntail/);
    const probe = await f.page.evaluate(() => ({ resets: window.outputProbe.resets, early: window.outputProbe.resetWhilePending }));
    assert.equal(probe.resets, 1);
    assert.equal(probe.early, false);
  });

  await test('unavailable snapshot falls back to the committed cursor without clearing history', async t => {
    const f = await fixture(t);
    f.disableSnapshots();
    await f.send(Buffer.concat([Buffer.from('keep-this-history\r\n'), Buffer.alloc(5 * 1024 * 1024), Buffer.from('tail\r\n')]));
    for (let attempt = 0; !f.connections[0].closed && attempt < 100; attempt += 1) await delay(25);
    assert.equal(f.connections[0].closed, true);
    await f.release();
    await connected(f, 3);
    assert.equal(f.connections[1].after, null);
    assert.ok(Number(f.connections[2].after) > 0);
    assert.ok(Number(f.connections[2].after) < f.offset());
    assert.match(await f.text(), /^seed-history\nkeep-this-history\ntail/);
    assert.equal(await f.page.evaluate(() => window.outputProbe.resets), 0);
  });

  await test('exit status cannot overtake output still parsing', async t => {
    const f = await fixture(t);
    const data = Buffer.from('last-output\r\n');
    await f.send(data, 4);
    await f.page.waitForFunction(bytes => window.outputProbe.bytes === bytes, data.length);
    f.connections[0].socket.send(JSON.stringify({ type: 'status', status: 'exited', exit_code: 0 }));
    await f.release();
    await f.page.waitForFunction(() => {
      const buffer = window.terminalHistoryPreview[0].buffer.normal;
      return Array.from({ length: buffer.length }, (_, i) => buffer.getLine(i).translateToString(true)).some(line => line.includes('[process exited'));
    });
    assert.match(await f.text(), /last-output\n\n\[process exited with code 0\]/);
  });
} finally {
  await browser?.close();
  await server.close();
}
