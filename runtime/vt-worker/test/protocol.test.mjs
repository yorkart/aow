import assert from 'node:assert/strict';
import { Readable } from 'node:stream';
import test from 'node:test';

import { MAX_DATA_BYTES, ProtocolError, failure, parseData, parseRequest, success } from '../src/protocol.mjs';
import { encodeFrame, MAX_FRAME_BYTES, PROTOCOL_VERSION, readFrames } from '../src/framing.mjs';

const header = (extra = {}) => ({ protocol_version: PROTOCOL_VERSION, id: 7, action: 'create', ...extra });

test('request headers require version, id and a known action, with payload outside JSON', () => {
  const request = header();
  assert.deepEqual(parseRequest(JSON.stringify(request)), { id: 7, action: 'create', request });
  for (const extra of [{ id: null }, { protocol_version: 0 }, { data: [1] }, { data_base64: 'AA==' }]) {
    assert.throws(() => parseRequest(JSON.stringify(header(extra))), errorWithCode('invalid_request'));
  }
  assert.throws(() => parseRequest('{bad json'), errorWithCode('invalid_json'));
  assert.throws(() => parseRequest(JSON.stringify(header({ action: 'unknown' }))), errorWithCode('action_not_found'));
});

test('failure of a one-way mutation identifies its session and generation', () => {
  assert.deepEqual(success('req-1', 'snapshot', { applied_offset: 3 }), {
    id: 'req-1', ok: true, action: 'snapshot', result: { applied_offset: 3 },
  });
  assert.deepEqual(failure(2, 'write', new ProtocolError('offset_mismatch', 'gap'), {
    session_id: 'pane', generation: 'epoch',
  }), {
    id: 2, ok: false, action: 'write', error: { code: 'offset_mismatch', message: 'gap' },
    session_id: 'pane', generation: 'epoch',
  });
});

test('write accepts arbitrary binary without a UTF-8 or base64 round trip', () => {
  const data = Buffer.from([0, 10, 13, 27, 128, 255]);
  assert.equal(parseData({ data }), data);
  assert.throws(() => parseData({ data: 'text' }), errorWithCode('invalid_params'));
  assert.throws(() => parseData({ data }, 2), errorWithCode('data_too_large'));
  assert.equal(MAX_DATA_BYTES, 8 * 1024 * 1024);
});

test('binary frames survive every split point and preserve message boundaries', async () => {
  const data = Buffer.from([0, 10, 13, 27, 128, 255]);
  const wire = Buffer.concat([encodeFrame(header(), data), encodeFrame(header({ id: 8 }))]);
  const expected = [{ metadata: JSON.stringify(header()), data }, {
    metadata: JSON.stringify(header({ id: 8 })), data: Buffer.alloc(0),
  }];
  for (let at = 1; at < wire.length; at++) {
    assert.deepEqual(await collect([wire.subarray(0, at), wire.subarray(at)]), expected);
  }
  assert.deepEqual(await collect([...wire].map(byte => Buffer.from([byte]))), expected);
});

test('oversized and inconsistent lengths are rejected from the prefix alone', async () => {
  for (const [length, metadataLength] of [[0, 0], [3, 1], [5, 2], [5, 0], [MAX_FRAME_BYTES + 1, 1], [0xffffffff, 1]]) {
    const prefix = Buffer.alloc(8);
    prefix.writeUInt32BE(length, 0);
    prefix.writeUInt32BE(metadataLength, 4);
    await assert.rejects(collect([prefix]), /invalid VT frame lengths/);
  }
  await assert.rejects(collect([encodeFrame(header())], { maxMetadataBytes: 4 }), /invalid VT frame lengths/);
});

test('EOF in either the prefix or the body is a fatal truncated frame', async () => {
  const frame = encodeFrame(header(), Buffer.from('payload'));
  assert.deepEqual(await collect([]), []);
  for (let length = 1; length < frame.length; length++) {
    await assert.rejects(collect([frame.subarray(0, length)]), /truncated VT frame/);
  }
});

async function collect(chunks, options) {
  const frames = [];
  for await (const frame of readFrames(Readable.from(chunks), options)) frames.push(frame);
  return frames;
}
function errorWithCode(code) {
  return error => error instanceof ProtocolError && error.code === code;
}
