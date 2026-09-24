import { encodeFrame, MAX_REQUEST_METADATA_BYTES, readFrames } from './framing.mjs';
import { ProtocolError, failure, parseRequest, success } from './protocol.mjs';
import { VtService } from './vt-service.mjs';

export async function runWorker({ input, output, logger = console, signal }) {
  const service = new VtService();
  const emit = (message, data) => writeOutput(output, encodeFrame(message, data));
  try {
    // Consume only one operation at a time. This preserves parse/resize
    // barriers and propagates pipe backpressure without an unbounded JS queue.
    for await (const frame of readFrames(input, { maxMetadataBytes: MAX_REQUEST_METADATA_BYTES })) {
      let parsed;
      let result;
      try {
        parsed = parseRequest(frame.metadata);
        if (parsed.action !== 'write' && frame.data.length !== 0) {
          throw new ProtocolError('invalid_params', 'only write accepts a binary payload');
        }
        result = await service.dispatch(parsed.action, { ...parsed.request, data: frame.data });
      } catch (error) {
        // Errors for one-way mutations carry their session/generation so the
        // daemon can invalidate that session even when it is not awaiting RPC.
        await emit(failure(parsed?.id, parsed?.action, error, parsed?.request));
        continue;
      }
      if (parsed.action === 'write' || parsed.action === 'resize') continue;
      const { data, ...metadata } = result;
      await emit(success(parsed.id, parsed.action, metadata), data);
    }
  } catch (error) {
    if (signal?.aborted) return;
    logger.error(`VT worker stream failure: ${error instanceof Error ? error.message : String(error)}`);
    throw error;
  } finally {
    service.disposeAll();
  }
}

function writeOutput(output, data) {
  return new Promise((resolve, reject) => {
    output.write(data, (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
}
