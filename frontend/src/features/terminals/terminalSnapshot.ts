// Keep the encoded cache budget and decoded-work bound aligned with vt-worker.
export const MAX_ENCODED_SNAPSHOT_BYTES = 8 * 1024 * 1024;
export const MAX_DECODED_SNAPSHOT_BYTES = 512 * 1024 * 1024;

export type CompressedTerminalSnapshot = { data: string; decoded_bytes: number };

export function isCompressedTerminalSnapshot(value: unknown): value is CompressedTerminalSnapshot {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const snapshot = value as Record<string, unknown>;
  return typeof snapshot.data === 'string'
    && snapshot.data.length > 0 && snapshot.data.length <= MAX_ENCODED_SNAPSHOT_BYTES
    && snapshot.data.length % 4 === 0 && /^[A-Za-z0-9+/]*={0,2}$/.test(snapshot.data)
    && typeof snapshot.decoded_bytes === 'number' && Number.isSafeInteger(snapshot.decoded_bytes)
    && snapshot.decoded_bytes > 0 && snapshot.decoded_bytes <= MAX_DECODED_SNAPSHOT_BYTES;
}

export async function writeCompressedTerminalSnapshot(
  snapshot: CompressedTerminalSnapshot,
  write: (bytes: Uint8Array) => Promise<void>,
  isCurrent: () => boolean,
) {
  const binary = atob(snapshot.data);
  const bytes = Uint8Array.from(binary, char => char.charCodeAt(0));
  const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();
  let decoded = 0;
  const batch = new Uint8Array(256 * 1024);
  let filled = 0;
  try {
    while (isCurrent()) {
      const { value, done } = await reader.read();
      if (!isCurrent()) return;
      if (done) {
        if (decoded !== snapshot.decoded_bytes) throw new Error('VT snapshot decoded length mismatch');
        if (filled) await write(batch.subarray(0, filled));
        return;
      }
      decoded += value.byteLength;
      if (decoded > snapshot.decoded_bytes || decoded > MAX_DECODED_SNAPSHOT_BYTES) {
        throw new Error('VT snapshot exceeds decoded byte limit');
      }
      // Decompressors may return tiny chunks. Coalesce them so xterm does not
      // pay a timer/render turn for each one, while retaining a fixed window.
      for (let offset = 0; offset < value.byteLength && isCurrent();) {
        const length = Math.min(batch.length - filled, value.byteLength - offset);
        batch.set(value.subarray(offset, offset + length), filled);
        filled += length;
        offset += length;
        if (filled === batch.length) {
          await write(batch);
          filled = 0;
        }
      }
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
