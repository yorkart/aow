import serializePackage from '@xterm/addon-serialize';
import headlessPackage from '@xterm/headless';

import {
  MAX_SNAPSHOT_BYTES,
  ProtocolError,
  parseData,
  parseDimensions,
  parseGeneration,
  parseOffset,
  parseSessionId,
} from './protocol.mjs';

const DEFAULT_SCROLLBACK = 100;
const MAX_SCROLLBACK = 100;
export const MAX_SESSIONS = 256;
export const MAX_SCREEN_CELLS = 1_000_000;
export const MAX_TOTAL_SCREEN_CELLS = 8_000_000;
const { SerializeAddon } = serializePackage;
const { Terminal } = headlessPackage;

export class VtService {
  #sessions = new Map();
  #tail = Promise.resolve();
  #maxDataBytes;
  #maxSnapshotBytes;
  #maxSessions;
  #maxScreenCells;
  #maxTotalScreenCells;
  #totalScreenCells = 0;

  constructor({
    maxDataBytes,
    maxSnapshotBytes = MAX_SNAPSHOT_BYTES,
    maxSessions = MAX_SESSIONS,
    maxScreenCells = MAX_SCREEN_CELLS,
    maxTotalScreenCells = MAX_TOTAL_SCREEN_CELLS,
  } = {}) {
    this.#maxDataBytes = maxDataBytes;
    this.#maxSnapshotBytes = maxSnapshotBytes;
    this.#maxSessions = maxSessions;
    this.#maxScreenCells = maxScreenCells;
    this.#maxTotalScreenCells = maxTotalScreenCells;
  }

  dispatch(action, request) {
    const operation = this.#tail.then(() => this.#dispatch(action, request));
    // A failed request must not poison the queue for subsequent requests.
    this.#tail = operation.catch(() => undefined);
    return operation;
  }

  disposeAll() {
    for (const session of this.#sessions.values()) {
      session.terminal.dispose();
    }
    this.#sessions.clear();
    this.#totalScreenCells = 0;
  }

  async #dispatch(action, request) {
    switch (action) {
      case 'create':
        return this.#create(request);
      case 'write':
        return this.#write(request);
      case 'resize':
        return this.#resize(request);
      case 'snapshot':
        return this.#snapshot(request);
      case 'dispose':
        return this.#dispose(request);
      default:
        throw new ProtocolError('action_not_found', `unknown action: ${action}`);
    }
  }

  #create(request) {
    const sessionId = parseSessionId(request);
    const generation = parseGeneration(request);
    const initialOffset = parseOffset(request, 'initial_offset');
    if (initialOffset !== 0) {
      throw new ProtocolError(
        'invalid_params',
        'initial_offset must be 0',
      );
    }
    const { cols, rows } = parseDimensions(request);
    const scrollback = parseScrollback(
      request.scrollback,
      DEFAULT_SCROLLBACK,
      MAX_SCROLLBACK,
    );
    const screenCells = this.#screenCells(cols, rows);

    if (this.#sessions.has(sessionId)) {
      throw new ProtocolError(
        'session_exists',
        `session already exists: ${sessionId}`,
      );
    }
    if (this.#sessions.size >= this.#maxSessions) {
      throw new ProtocolError(
        'session_limit_exceeded',
        `session count must not exceed ${this.#maxSessions}`,
      );
    }
    this.#assertTotalScreenCells(this.#totalScreenCells + screenCells);

    const terminal = new Terminal({
      cols,
      rows,
      scrollback,
      // addon-serialize reads the public buffer API, which xterm 6 currently
      // marks as proposed in the headless package.
      allowProposedApi: true,
      logLevel: 'off',
    });
    const serializer = new SerializeAddon();
    terminal.loadAddon(serializer);
    this.#sessions.set(sessionId, {
      terminal,
      serializer,
      generation,
      appliedOffset: initialOffset,
      scrollback,
      screenCells,
    });
    this.#totalScreenCells += screenCells;

    return stateResult(sessionId, generation, initialOffset, {
      cols,
      rows,
      scrollback,
    });
  }

  async #write(request) {
    const sessionId = parseSessionId(request);
    const generation = parseGeneration(request);
    const startOffset = parseOffset(request, 'start_offset');
    const session = this.#getSession(sessionId, generation);

    if (startOffset !== session.appliedOffset) {
      throw new ProtocolError(
        'offset_mismatch',
        `start_offset ${startOffset} does not match applied_offset ${session.appliedOffset}`,
      );
    }
    const data = parseData(request, this.#maxDataBytes);
    const appliedOffset = startOffset + data.byteLength;
    if (!Number.isSafeInteger(appliedOffset)) {
      throw new ProtocolError('offset_overflow', 'applied_offset exceeds safe integer range');
    }

    // Completion is local to the worker; writes have no success ACK on the
    // wire. Later resize/snapshot operations must wait for this callback.
    await writeTerminal(session.terminal, data);
    session.appliedOffset = appliedOffset;

    return stateResult(sessionId, generation, appliedOffset, {
      byte_length: data.byteLength,
    });
  }

  #resize(request) {
    const sessionId = parseSessionId(request);
    const generation = parseGeneration(request);
    const session = this.#getSession(sessionId, generation);
    const atOffset = parseOffset(request, 'at_offset');
    const { cols, rows } = parseDimensions(request);
    const screenCells = this.#screenCells(cols, rows);
    if (atOffset !== session.appliedOffset) {
      throw new ProtocolError(
        'offset_mismatch',
        `at_offset ${atOffset} does not match applied_offset ${session.appliedOffset}`,
      );
    }
    const nextTotalScreenCells = this.#totalScreenCells
      - session.screenCells
      + screenCells;
    this.#assertTotalScreenCells(nextTotalScreenCells);

    session.terminal.resize(cols, rows);
    session.screenCells = screenCells;
    this.#totalScreenCells = nextTotalScreenCells;
    return stateResult(sessionId, generation, session.appliedOffset, { cols, rows });
  }

  #snapshot(request) {
    const sessionId = parseSessionId(request);
    const generation = parseGeneration(request);
    const session = this.#getSession(sessionId, generation);
    const scrollback = parseScrollback(
      request.scrollback,
      session.scrollback,
      session.scrollback,
    );
    const serialized = session.serializer.serialize({ scrollback });
    const data = Buffer.from(serialized, 'utf8');

    if (data.byteLength > this.#maxSnapshotBytes) {
      throw new ProtocolError(
        'snapshot_too_large',
        `serialized snapshot exceeds the ${this.#maxSnapshotBytes} byte limit`,
      );
    }
    return stateResult(sessionId, generation, session.appliedOffset, {
      cols: session.terminal.cols,
      rows: session.terminal.rows,
      lines: Array.from({ length: session.terminal.rows }, (_, row) =>
        session.terminal.buffer.active.getLine(session.terminal.buffer.active.baseY + row)?.translateToString(true) ?? ''),
      data,
      byte_length: data.byteLength,
    });
  }

  #dispose(request) {
    const sessionId = parseSessionId(request);
    const generation = parseGeneration(request);
    const session = this.#getSession(sessionId, generation);
    const appliedOffset = session.appliedOffset;

    session.terminal.dispose();
    this.#sessions.delete(sessionId);
    this.#totalScreenCells -= session.screenCells;
    return stateResult(sessionId, generation, appliedOffset, { disposed: true });
  }

  #getSession(sessionId, generation) {
    const session = this.#sessions.get(sessionId);
    if (!session) {
      throw new ProtocolError('session_not_found', `unknown session: ${sessionId}`);
    }
    if (session.generation !== generation) {
      throw new ProtocolError(
        'generation_mismatch',
        `generation does not match session ${sessionId}`,
      );
    }
    return session;
  }

  #screenCells(cols, rows) {
    const screenCells = cols * rows;
    if (screenCells > this.#maxScreenCells) {
      throw new ProtocolError(
        'screen_limit_exceeded',
        `cols * rows must not exceed ${this.#maxScreenCells}`,
      );
    }
    return screenCells;
  }

  #assertTotalScreenCells(totalScreenCells) {
    if (totalScreenCells > this.#maxTotalScreenCells) {
      throw new ProtocolError(
        'screen_limit_exceeded',
        `total screen cells must not exceed ${this.#maxTotalScreenCells}`,
      );
    }
  }
}

function parseScrollback(value, fallback, maximum) {
  if (value === undefined) {
    return fallback;
  }
  if (!Number.isInteger(value) || value < 0 || value > maximum) {
    throw new ProtocolError(
      'invalid_params',
      `scrollback must be an integer between 0 and ${maximum}`,
    );
  }
  return value;
}

function stateResult(sessionId, generation, appliedOffset, extra = {}) {
  return {
    session_id: sessionId,
    generation,
    applied_offset: appliedOffset,
    ...extra,
  };
}

function writeTerminal(terminal, data) {
  return new Promise((resolve) => terminal.write(data, resolve));
}
