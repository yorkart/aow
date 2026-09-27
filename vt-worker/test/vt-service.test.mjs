import assert from 'node:assert/strict';
import test from 'node:test';
import headlessPackage from '@xterm/headless';

import { ProtocolError } from '../src/protocol.mjs';
import {
  MAX_SCREEN_CELLS,
  MAX_SESSIONS,
  MAX_TOTAL_SCREEN_CELLS,
  VtService,
} from '../src/vt-service.mjs';

const { Terminal } = headlessPackage;

const SESSION = 'pane-1';
const GENERATION = 'epoch-1';

function request(action, extra = {}) {
  return {
    id: action,
    action,
    session_id: SESSION,
    generation: GENERATION,
    ...extra,
  };
}

async function create(service, extra = {}) {
  return service.dispatch('create', request('create', {
    cols: 20,
    rows: 5,
    scrollback: 100,
    initial_offset: 0,
    ...extra,
  }));
}

test('create, ordered write, resize, snapshot, and dispose preserve state', async () => {
  const service = new VtService();
  try {
    assert.deepEqual(await create(service), {
      session_id: SESSION,
      generation: GENERATION,
      applied_offset: 0,
      cols: 20,
      rows: 5,
      scrollback: 100,
    });

    const bytes = Buffer.from('hello\r\n\u001b[31mred\u001b[0m');
    const write = await service.dispatch('write', request('write', {
      start_offset: 0,
      data: bytes,
    }));
    assert.equal(write.applied_offset, bytes.byteLength);
    assert.equal(write.byte_length, bytes.byteLength);

    const resize = await service.dispatch('resize', request('resize', {
      at_offset: bytes.byteLength,
      cols: 30,
      rows: 8,
    }));
    assert.equal(resize.applied_offset, bytes.byteLength);
    assert.equal(resize.cols, 30);

    const snapshot = await service.dispatch('snapshot', request('snapshot'));
    assert.equal(snapshot.generation, GENERATION);
    assert.equal(snapshot.applied_offset, bytes.byteLength);
    assert.equal(snapshot.cols, 30);
    assert.equal(snapshot.rows, 8);
    assert.ok(snapshot.byte_length > 0);
    assert.ok(snapshot.data.includes('hello'));

    const restored = await restoreSnapshot(snapshot, 100);
    try {
      assert.equal(restored.cols, 30);
      assert.equal(restored.rows, 8);
      assert.equal(restored.buffer.active.type, 'normal');
      assert.match(bufferText(restored.buffer.normal), /hello/);
      assert.match(bufferText(restored.buffer.normal), /red/);
    } finally {
      restored.dispose();
    }

    const disposed = await service.dispatch('dispose', request('dispose'));
    assert.equal(disposed.disposed, true);
    await assert.rejects(
      service.dispatch('snapshot', request('snapshot')),
      errorWithCode('session_not_found'),
    );
  } finally {
    service.disposeAll();
  }
});

test('viewport lines follow redraws, scrollback and the active screen', async () => {
  const service = new VtService();
  let offset = 0;
  const write = async text => {
    const bytes = Buffer.from(text);
    await service.dispatch('write', request('write', { start_offset: offset, data: bytes }));
    offset += bytes.length;
    return (await service.dispatch('snapshot', request('snapshot'))).lines;
  };
  try {
    await create(service, { cols: 40, rows: 5 });
    assert.equal((await write('\x1b[5;1Hmodel · cwd · ready'))[4], 'model · cwd · ready');
    assert.equal((await write('\r\x1b[2Kmodel: loading'))[4], 'model: loading');
    const scrolled = await write('\r\n1\r\n2\r\n3\r\n4\r\n5');
    assert.deepEqual(scrolled, ['1', '2', '3', '4', '5']);
    assert.deepEqual(await write('\x1b[?1049h\x1b[2J\x1b[Hstatus panel'), ['status panel', '', '', '', '']);
    assert.deepEqual(await write('\x1b[?1049l'), scrolled);
  } finally { service.disposeAll(); }
});

test('snapshot ANSI restores normal/alternate screens and terminal modes', async () => {
  const service = new VtService();
  try {
    await create(service);
    const bytes = Buffer.from(
      'primary\r\n'
      + '\u001b[?1049h\u001b[2J\u001b[H'
      + '\u001b[?1h\u001b[?2004h'
      + '\u001b[38;2;1;2;3malternate\u001b[0m',
    );
    await service.dispatch('write', request('write', {
      start_offset: 0,
      data: bytes,
    }));
    const snapshot = await service.dispatch('snapshot', request('snapshot'));
    const ansi = snapshot.data.toString('utf8');
    assert.match(ansi, /alternate/);
    assert.match(ansi, /\u001b\[\?1049h/);
    assert.equal(snapshot.applied_offset, bytes.byteLength);

    const restored = await restoreSnapshot(snapshot, 100);
    try {
      assert.equal(restored.buffer.active.type, 'alternate');
      assert.match(bufferText(restored.buffer.normal), /primary/);
      assert.match(bufferText(restored.buffer.alternate), /alternate/);
      assert.equal(restored.modes.applicationCursorKeysMode, true);
      assert.equal(restored.modes.bracketedPasteMode, true);
    } finally {
      restored.dispose();
    }
  } finally {
    service.disposeAll();
  }
});

test('snapshot cannot overtake an earlier write', async () => {
  const service = new VtService();
  try {
    await create(service);
    const bytes = Buffer.from('queued-before-snapshot');
    const writePromise = service.dispatch('write', request('write', {
      start_offset: 0,
      data: bytes,
    }));
    const snapshotPromise = service.dispatch('snapshot', request('snapshot'));
    const [write, snapshot] = await Promise.all([writePromise, snapshotPromise]);
    assert.equal(write.applied_offset, bytes.byteLength);
    assert.equal(snapshot.applied_offset, bytes.byteLength);
    assert.ok(
      snapshot.data.includes('queued-before-snapshot'),
    );
  } finally {
    service.disposeAll();
  }
});

test('write rejects gaps and does not advance applied_offset', async () => {
  const service = new VtService();
  try {
    await create(service);
    await assert.rejects(
      service.dispatch('write', request('write', {
        start_offset: 9,
        data: Buffer.from('bad'),
      })),
      errorWithCode('offset_mismatch'),
    );
    const snapshot = await service.dispatch('snapshot', request('snapshot'));
    assert.equal(snapshot.applied_offset, 0);
  } finally {
    service.disposeAll();
  }
});

test('generation mismatch is rejected for every existing-session operation', async () => {
  const service = new VtService();
  try {
    await create(service);
    const staleRequests = [
      ['write', {
        start_offset: 0,
        data: Buffer.from('stale'),
      }],
      ['resize', { at_offset: 0, cols: 21, rows: 5 }],
      ['snapshot', {}],
      ['dispose', {}],
    ];
    for (const [action, extra] of staleRequests) {
      await assert.rejects(
        service.dispatch(action, request(action, {
          ...extra,
          generation: 'epoch-old',
        })),
        errorWithCode('generation_mismatch'),
      );
    }

    // In particular, a stale dispose must not remove the live generation.
    const snapshot = await service.dispatch('snapshot', request('snapshot'));
    assert.equal(snapshot.generation, GENERATION);
  } finally {
    service.disposeAll();
  }
});

test('resize validates at_offset before mutating the terminal', async () => {
  const service = new VtService();
  try {
    await create(service);
    await assert.rejects(
      service.dispatch('resize', request('resize', {
        at_offset: 1, cols: 40, rows: 10,
      })),
      errorWithCode('offset_mismatch'),
    );
    const snapshot = await service.dispatch('snapshot', request('snapshot'));
    assert.equal(snapshot.cols, 20);
    assert.equal(snapshot.rows, 5);
  } finally {
    service.disposeAll();
  }
});

test('create requires a zero initial offset and bounds scrollback', async () => {
  const service = new VtService();
  try {
    await assert.rejects(
      create(service, { initial_offset: 1 }),
      errorWithCode('invalid_params'),
    );
    await assert.rejects(
      create(service, { initial_offset: Number.MAX_SAFE_INTEGER }),
      errorWithCode('invalid_params'),
    );
    await assert.rejects(
      service.dispatch('create', request('create', {
        cols: 20,
        rows: 5,
        scrollback: 100,
        initial_offset: undefined,
      })),
      errorWithCode('invalid_params'),
    );
    await assert.rejects(
      create(service, { session_id: 'bad-scrollback', scrollback: 101 }),
      errorWithCode('invalid_params'),
    );
  } finally {
    service.disposeAll();
  }
});

test('session limit is bounded and dispose releases capacity', async () => {
  assert.equal(MAX_SESSIONS, 256);
  const service = new VtService({
    maxSessions: 2,
    maxTotalScreenCells: 1_000,
  });
  try {
    await create(service, { session_id: 'session-a' });
    await create(service, { session_id: 'session-b' });
    await assert.rejects(
      create(service, { session_id: 'session-c', cols: 1, rows: 1 }),
      errorWithCode('session_limit_exceeded'),
    );
    await service.dispatch('dispose', request('dispose', { session_id: 'session-a' }));
    const created = await create(service, {
      session_id: 'session-c',
      cols: 1,
      rows: 1,
    });
    assert.equal(created.session_id, 'session-c');
  } finally {
    service.disposeAll();
  }
});

test('per-session and aggregate screen-cell limits account for resize/dispose', async () => {
  assert.equal(MAX_SCREEN_CELLS, 1_000_000);
  assert.equal(MAX_TOTAL_SCREEN_CELLS, 8_000_000);
  const service = new VtService({
    maxSessions: 10,
    maxScreenCells: 100,
    maxTotalScreenCells: 100,
  });
  try {
    await assert.rejects(
      create(service, { session_id: 'too-large', cols: 11, rows: 10 }),
      errorWithCode('screen_limit_exceeded'),
    );
    await create(service, { session_id: 'screen-a', cols: 10, rows: 6 });
    await create(service, { session_id: 'screen-b', cols: 10, rows: 4 });

    await assert.rejects(
      service.dispatch('resize', request('resize', {
        session_id: 'screen-b', at_offset: 0, cols: 10, rows: 5,
      })),
      errorWithCode('screen_limit_exceeded'),
    );
    const unchanged = await service.dispatch(
      'snapshot',
      request('snapshot', { session_id: 'screen-b' }),
    );
    assert.equal(unchanged.rows, 4);

    await service.dispatch('resize', request('resize', {
      session_id: 'screen-a', at_offset: 0, cols: 10, rows: 3,
    }));
    await service.dispatch('resize', request('resize', {
      session_id: 'screen-b', at_offset: 0, cols: 10, rows: 5,
    }));
    await service.dispatch('dispose', request('dispose', { session_id: 'screen-a' }));
    const created = await create(service, {
      session_id: 'screen-c', cols: 10, rows: 5,
    });
    assert.equal(created.rows, 5);
    await assert.rejects(
      create(service, { session_id: 'over-total', cols: 1, rows: 1 }),
      errorWithCode('screen_limit_exceeded'),
    );
  } finally {
    service.disposeAll();
  }
});

test('one-column requests report effective geometry and charge actual screen cells', async () => {
  const service = new VtService({ maxScreenCells: 4, maxTotalScreenCells: 4 });
  try {
    await assert.rejects(create(service, { cols: 0, rows: 1 }), errorWithCode('invalid_params'));
    await assert.rejects(create(service, { cols: 1, rows: 3 }), errorWithCode('screen_limit_exceeded'));
    const created = await create(service, { cols: 1, rows: 1 });
    assert.deepEqual([created.cols, created.rows], [2, 1]);
    const initial = await service.dispatch('snapshot', request('snapshot'));
    assert.deepEqual([initial.cols, initial.rows], [2, 1]);

    // The first session occupies two cells, leaving only two for other sessions.
    await assert.rejects(
      create(service, { session_id: 'other', cols: 3, rows: 1 }),
      errorWithCode('screen_limit_exceeded'),
    );
    await create(service, { session_id: 'other', cols: 2, rows: 1 });
    await assert.rejects(
      service.dispatch('resize', request('resize', { at_offset: 0, cols: 1, rows: 2 })),
      errorWithCode('screen_limit_exceeded'),
    );
    await service.dispatch('dispose', request('dispose', { session_id: 'other' }));
    const resized = await service.dispatch('resize', request('resize', { at_offset: 0, cols: 1, rows: 2 }));
    const snapshot = await service.dispatch('snapshot', request('snapshot'));
    assert.deepEqual([resized.cols, resized.rows], [2, 2]);
    assert.deepEqual([snapshot.cols, snapshot.rows], [2, 2]);

    await service.dispatch('dispose', request('dispose'));
    await create(service, { cols: 2, rows: 2 });
  } finally {
    service.disposeAll();
  }
});

test('worker enforces per-session and aggregate screen budgets', async () => {
  const service = new VtService({
    maxSessions: 2,
    maxScreenCells: 100,
    maxTotalScreenCells: 120,
  });
  try {
    await create(service, { cols: 10, rows: 5 });
    await assert.rejects(
      service.dispatch('resize', request('resize', {
        at_offset: 0, cols: 11, rows: 10,
      })),
      errorWithCode('screen_limit_exceeded'),
    );

    await service.dispatch('create', {
      id: 'create-2',
      action: 'create',
      session_id: 'pane-2',
      generation: 'epoch-2',
      cols: 10,
      rows: 7,
      scrollback: 100,
      initial_offset: 0,
    });
    await assert.rejects(
      service.dispatch('create', {
        id: 'create-3',
        action: 'create',
        session_id: 'pane-3',
        generation: 'epoch-3',
        cols: 1,
        rows: 1,
        scrollback: 100,
        initial_offset: 0,
      }),
      errorWithCode('session_limit_exceeded'),
    );
  } finally {
    service.disposeAll();
  }
});

test('snapshot limit produces a structured error condition', async () => {
  const service = new VtService({ maxSnapshotBytes: 8 });
  try {
    await create(service);
    const bytes = Buffer.from('this is more than eight bytes');
    await service.dispatch('write', request('write', {
      start_offset: 0,
      data: bytes,
    }));
    await assert.rejects(
      service.dispatch('snapshot', request('snapshot')),
      errorWithCode('snapshot_too_large'),
    );
    const disposed = await service.dispatch('dispose', request('dispose'));
    assert.equal(disposed.disposed, true);
  } finally {
    service.disposeAll();
  }
});

function errorWithCode(code) {
  return (error) => error instanceof ProtocolError && error.code === code;
}

async function restoreSnapshot(snapshot, scrollback) {
  const terminal = new Terminal({
    cols: snapshot.cols,
    rows: snapshot.rows,
    scrollback,
    allowProposedApi: true,
    logLevel: 'off',
  });
  await writeTerminal(terminal, snapshot.data);
  return terminal;
}

function writeTerminal(terminal, data) {
  return new Promise((resolve) => terminal.write(data, resolve));
}

function bufferText(buffer) {
  const lines = [];
  for (let index = 0; index < buffer.length; index += 1) {
    lines.push(buffer.getLine(index)?.translateToString(true) ?? '');
  }
  return lines.join('\n');
}
