import assert from 'node:assert/strict';
import { test } from 'node:test';
import { gzipSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)),
  optimizeDeps: { noDiscovery: true, entries: [] }, server: { middlewareMode: true } });
try {
  const { isCompressedTerminalSnapshot, writeCompressedTerminalSnapshot } = await server.ssrLoadModule('/src/features/terminals/terminalSnapshot.ts');
  const pack = text => ({ data: gzipSync(text).toString('base64'), decoded_bytes: Buffer.byteLength(text) });

  await test('compressed restore streams exact UTF-8 and waits for the parser before more writes', async () => {
    const original = '中文\x1b[31mcolored\x1b[0m\r\n'.repeat(40_000);
    const writes = [], gate = Promise.withResolvers(), started = Promise.withResolvers();
    const restoring = writeCompressedTerminalSnapshot(pack(original), async bytes => {
      writes.push(Buffer.from(bytes));
      started.resolve();
      await gate.promise;
    }, () => true);
    await started.promise;
    assert.equal(writes.length, 1);
    assert.ok(writes[0].length <= 256 * 1024);
    gate.resolve();
    await restoring;
    assert.equal(Buffer.concat(writes).toString(), original);
  });

  await test('compressed restore rejects corrupt data, false lengths and oversized declarations', async () => {
    const snapshot = pack('exact output');
    for (const bad of [null, [], {}, { ...snapshot, data: 'bad!' },
      { ...snapshot, decoded_bytes: -1 }, { ...snapshot, decoded_bytes: 512 * 1024 * 1024 + 1 }]) {
      assert.equal(isCompressedTerminalSnapshot(bad), false);
    }
    for (const bad of [{ ...snapshot, decoded_bytes: 1 }, { ...snapshot, decoded_bytes: 100 },
      { ...snapshot, data: Buffer.from('not gzip').toString('base64') }]) {
      await assert.rejects(writeCompressedTerminalSnapshot(bad, async () => {}, () => true));
    }
  });

  await test('detaching a pane cancels decompression without submitting later writes', async () => {
    let current = true, writes = 0;
    await writeCompressedTerminalSnapshot(pack('x'.repeat(1024 * 1024)), async () => {
      writes += 1;
      current = false;
    }, () => current);
    assert.equal(writes, 1);
  });
} finally { await server.close(); }
