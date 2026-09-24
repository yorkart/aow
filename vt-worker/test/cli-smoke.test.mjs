import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { encodeFrame, PROTOCOL_VERSION, readFrames } from '../src/framing.mjs';

const BUNDLE_PATH = fileURLToPath(new URL('../dist/vt-worker.mjs', import.meta.url));

test('bundled binary worker pipelines writes/resizes without ACK and keeps snapshot barriers', { timeout: 10000 }, async (t) => {
  const child = spawn(process.execPath, [BUNDLE_PATH], { stdio: ['pipe', 'pipe', 'pipe'] });
  const exited = once(child, 'exit');
  t.after(() => { if (child.exitCode === null) child.kill('SIGKILL'); });
  let stderr = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', chunk => { stderr += chunk; });
  const responses = readFrames(child.stdout)[Symbol.asyncIterator]();
  const send = async (metadata, data) => {
    if (!child.stdin.write(encodeFrame(metadata, data))) await once(child.stdin, 'drain');
  };
  const receive = async (id) => {
    const { value, done } = await responses.next();
    assert.equal(done, false, `worker exited early: ${stderr}`);
    const metadata = JSON.parse(value.metadata);
    assert.equal(metadata.protocol_version, PROTOCOL_VERSION);
    assert.equal(metadata.id, id, 'successful write/resize must not emit ACKs');
    return { ...metadata, data: value.data };
  };
  const base = { session_id: 'cli-pane', generation: 'cli-epoch' };
  await send({ id: 1, action: 'create', ...base, cols: 40, rows: 8, initial_offset: 0 });
  assert.equal((await receive(1)).ok, true);

  // Split ANSI and multibyte UTF-8 across many unacknowledged writes.
  const bytes = Buffer.from('cli smoke\r\n\x1b[?1049h\x1b[2J\x1b[H你好 fullscreen');
  for (let offset = 0; offset < bytes.length; offset++) {
    await send({ id: 10 + offset, action: 'write', ...base, start_offset: offset }, bytes.subarray(offset, offset + 1));
  }
  await send({ id: 100, action: 'snapshot', ...base });
  const before = await receive(100);
  assert.equal(before.ok, true);
  assert.equal(before.result.applied_offset, bytes.length);
  assert.deepEqual([before.result.cols, before.result.rows], [40, 8]);
  assert.ok(before.data.includes('cli smoke'));
  assert.ok(before.data.includes('你好 fullscreen'));
  assert.equal(before.result.byte_length, before.data.length);
  assert.equal('data_base64' in before.result, false);

  await send({ id: 101, action: 'resize', ...base, at_offset: bytes.length, cols: 60, rows: 10 });
  await send({ id: 102, action: 'write', ...base, start_offset: bytes.length }, Buffer.from('\r\nafter resize'));
  await send({ id: 103, action: 'snapshot', ...base });
  const after = await receive(103);
  assert.equal(after.result.applied_offset, bytes.length + 14);
  assert.deepEqual([after.result.cols, after.result.rows], [60, 10]);
  assert.equal(after.result.lines[1], 'after resize');

  // A rejected one-way mutation reports an error immediately, without a query.
  await send({ id: 104, action: 'write', ...base, start_offset: 99999 }, Buffer.from('bad'));
  const error = await receive(104);
  assert.equal(error.ok, false);
  assert.equal(error.error.code, 'offset_mismatch');
  assert.equal(error.session_id, base.session_id);
  assert.equal(error.generation, base.generation);
  await send({ id: 105, action: 'snapshot', ...base });
  assert.equal((await receive(105)).result.applied_offset, bytes.length + 14);
  await send({ id: 106, action: 'dispose', ...base });
  assert.equal((await receive(106)).result.disposed, true);
  child.kill('SIGTERM');
  const [code, signal] = await exited;
  assert.equal(signal, null);
  assert.equal(code, 0, stderr);
  assert.equal(stderr, '');
});
