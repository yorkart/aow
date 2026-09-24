// Wire v1: u32be body length, u32be JSON length, JSON, then opaque bytes.
// Body length includes the JSON-length field, but excludes its own field.
export const PROTOCOL_VERSION = 1;
export const MAX_FRAME_BYTES = 16 * 1024 * 1024 + 64 * 1024;
export const MAX_REQUEST_METADATA_BYTES = 64 * 1024;

export function encodeFrame(metadata, data = Buffer.alloc(0)) {
  const json = Buffer.from(JSON.stringify({ ...metadata, protocol_version: PROTOCOL_VERSION }));
  const bodyLength = 4 + json.length + data.byteLength;
  if (bodyLength > MAX_FRAME_BYTES) throw new Error('VT frame exceeds size limit');
  const prefix = Buffer.allocUnsafe(8);
  prefix.writeUInt32BE(bodyLength, 0);
  prefix.writeUInt32BE(json.length, 4);
  return Buffer.concat([prefix, json, data]);
}

export async function* readFrames(input, {
  maxFrameBytes = MAX_FRAME_BYTES,
  maxMetadataBytes = maxFrameBytes - 4,
} = {}) {
  const prefix = Buffer.allocUnsafe(8);
  let prefixLength = 0;
  let body;
  let bodyLength = 0;
  let metadataLength = 0;
  for await (const rawChunk of input) {
    const chunk = Buffer.isBuffer(rawChunk) ? rawChunk : Buffer.from(rawChunk);
    let offset = 0;
    while (offset < chunk.length) {
      if (prefixLength < 8) {
        const count = Math.min(8 - prefixLength, chunk.length - offset);
        chunk.copy(prefix, prefixLength, offset, offset + count);
        prefixLength += count;
        offset += count;
        if (prefixLength < 8) continue;
        const frameLength = prefix.readUInt32BE(0);
        metadataLength = prefix.readUInt32BE(4);
        // Validate before allocating. Invalid framing is fatal: never guess
        // where the next frame begins, or buffer an unbounded declared body.
        if (frameLength < 4 || frameLength > maxFrameBytes
          || metadataLength === 0 || metadataLength > maxMetadataBytes
          || metadataLength > frameLength - 4) {
          throw new Error('invalid VT frame lengths');
        }
        body = Buffer.allocUnsafe(frameLength - 4);
        bodyLength = 0;
      }
      const count = Math.min(body.length - bodyLength, chunk.length - offset);
      chunk.copy(body, bodyLength, offset, offset + count);
      bodyLength += count;
      offset += count;
      if (bodyLength === body.length) {
        const frame = {
          metadata: body.subarray(0, metadataLength).toString('utf8'),
          data: body.subarray(metadataLength),
        };
        prefixLength = 0;
        body = undefined;
        yield frame;
      }
    }
  }
  if (prefixLength !== 0) throw new Error('truncated VT frame');
}
