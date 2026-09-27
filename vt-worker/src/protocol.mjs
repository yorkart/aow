import { PROTOCOL_VERSION } from './framing.mjs';

export const MAX_DATA_BYTES = 8 * 1024 * 1024;
export const MAX_SNAPSHOT_BYTES = 2 * 1024 * 1024;

// Keep this aligned with terminald's public terminal dimension contract.
const MAX_DIMENSION = 1_000;
const MAX_SESSION_ID_BYTES = 1_024;
const MAX_GENERATION_BYTES = 1_024;

export class ProtocolError extends Error {
  constructor(code, message, context = {}) {
    super(message);
    this.name = 'ProtocolError';
    this.code = code;
    this.responseId = context.id ?? null;
    this.responseAction = context.action ?? null;
  }
}

export function parseRequest(line) {
  let value;
  try {
    value = JSON.parse(line);
  } catch {
    throw new ProtocolError('invalid_json', 'request must be valid JSON');
  }

  if (!isRecord(value)) {
    throw new ProtocolError('invalid_request', 'request must be a JSON object');
  }
  const id = parseId(value.id);
  if (value.protocol_version !== PROTOCOL_VERSION) {
    throw new ProtocolError('invalid_request', 'unsupported VT protocol version', { id });
  }
  if ('data' in value || 'data_base64' in value) {
    throw new ProtocolError('invalid_request', 'data must be a binary frame payload', { id });
  }
  if (typeof value.action !== 'string' || value.action.length === 0) {
    throw new ProtocolError(
      'invalid_request',
      'action must be a non-empty string',
      { id },
    );
  }
  if (Buffer.byteLength(value.action, 'utf8') > 64) {
    throw new ProtocolError(
      'invalid_request',
      'action must not exceed 64 UTF-8 bytes',
      { id },
    );
  }
  if (!KNOWN_ACTIONS.has(value.action)) {
    throw new ProtocolError(
      'action_not_found',
      `unknown action: ${value.action}`,
      { id, action: value.action },
    );
  }

  return { id, action: value.action, request: value };
}

const KNOWN_ACTIONS = new Set([
  'create',
  'write',
  'resize',
  'snapshot',
  'dispose',
]);

export function parseDimensions(params) {
  return {
    // Match xterm's minimum width before reporting geometry or charging screen
    // cells. Keep aligned with normalize_vt_cols in terminald's vt_worker.rs.
    cols: Math.max(2, parseDimension(params.cols, 'cols')),
    rows: parseDimension(params.rows, 'rows'),
  };
}

export function parseSessionId(params) {
  return parseBoundedString(
    params.session_id,
    'session_id',
    MAX_SESSION_ID_BYTES,
  );
}

export function parseGeneration(params) {
  return parseBoundedString(
    params.generation,
    'generation',
    MAX_GENERATION_BYTES,
  );
}

export function parseOffset(params, name) {
  const value = params[name];
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new ProtocolError(
      'invalid_params',
      `${name} must be a non-negative safe integer`,
    );
  }
  return value;
}

export function parseData(params, maxBytes = MAX_DATA_BYTES) {
  maxBytes ??= MAX_DATA_BYTES;
  const data = params.data;
  if (!(data instanceof Uint8Array)) {
    throw new ProtocolError('invalid_params', 'data must be binary bytes');
  }
  if (data.byteLength > maxBytes) {
    throw new ProtocolError(
      'data_too_large',
      `data exceeds the ${maxBytes} byte limit`,
    );
  }
  return data;
}

export function success(id, action, result) {
  return { id, ok: true, action, result };
}

export function failure(id, action, error, request) {
  const normalized = normalizeError(error);
  return {
    id: id ?? normalized.responseId ?? null,
    ok: false,
    action: action ?? normalized.responseAction ?? null,
    error: { code: normalized.code, message: normalized.message },
    ...(request ? { session_id: request.session_id, generation: request.generation } : {}),
  };
}

function parseId(id) {
  const validString = typeof id === 'string' && id.length > 0;
  const validInteger = Number.isSafeInteger(id) && id >= 0;
  if (!validString && !validInteger) {
    throw new ProtocolError(
      'invalid_request',
      'id must be a non-empty string or a non-negative safe integer',
    );
  }
  if (validString && Buffer.byteLength(id, 'utf8') > 1_024) {
    throw new ProtocolError(
      'invalid_request',
      'string id must not exceed 1024 UTF-8 bytes',
    );
  }
  return id;
}

function parseDimension(value, name) {
  if (!Number.isInteger(value) || value < 1 || value > MAX_DIMENSION) {
    throw new ProtocolError(
      'invalid_params',
      `${name} must be an integer between 1 and ${MAX_DIMENSION}`,
    );
  }
  return value;
}

function parseBoundedString(value, name, maxBytes) {
  if (typeof value !== 'string' || value.length === 0) {
    throw new ProtocolError(
      'invalid_params',
      `${name} must be a non-empty string`,
    );
  }
  if (Buffer.byteLength(value, 'utf8') > maxBytes) {
    throw new ProtocolError(
      'invalid_params',
      `${name} must not exceed ${maxBytes} UTF-8 bytes`,
    );
  }
  return value;
}

function normalizeError(error) {
  if (error instanceof ProtocolError) {
    return error;
  }
  const message = error instanceof Error ? error.message : String(error);
  return new ProtocolError('internal_error', message);
}

function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}
