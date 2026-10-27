import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
  optimizeDeps: { noDiscovery: true, entries: [] }, server: { middlewareMode: true } });
try {
  const { TerminalOutputQueue } = await server.ssrLoadModule('/src/features/terminals/terminalOutputQueue.ts');
  await test('plain writes pipeline while reset and subsequent writes wait for commits', async () => {
    const queue = new TerminalOutputQueue();
    const events = [];
    const first = Promise.withResolvers(), second = Promise.withResolvers(), reset = Promise.withResolvers();
    const resetStarted = Promise.withResolvers();
    const a = queue.write(() => { events.push('first'); return first.promise; });
    const b = queue.write(() => { events.push('second'); return second.promise; });
    const barrier = queue.barrier(() => { events.push('reset'); resetStarted.resolve(); return reset.promise; });
    const last = queue.write(() => events.push('after-reset'));
    await Promise.resolve();
    assert.deepEqual(events, ['first', 'second']);
    first.resolve();
    await a;
    assert.deepEqual(events, ['first', 'second']);
    second.resolve();
    await b;
    await resetStarted.promise;
    assert.deepEqual(events, ['first', 'second', 'reset']);
    reset.resolve();
    await barrier;
    await last;
    assert.deepEqual(events, ['first', 'second', 'reset', 'after-reset']);
    await queue.idle();
  });

  await test('a failed or skipped later write cannot release an earlier pending write barrier', async () => {
    const queue = new TerminalOutputQueue();
    const pending = Promise.withResolvers();
    let reset = false;
    const first = queue.write(() => pending.promise);
    const failed = queue.write(() => { throw new Error('write failed'); });
    const skipped = queue.write(() => {});
    const barrier = queue.barrier(() => { reset = true; });
    await assert.rejects(failed, /write failed/);
    await skipped;
    assert.equal(reset, false);
    pending.resolve();
    await first;
    await barrier;
    assert.equal(reset, true);
  });
} finally { await server.close(); }
