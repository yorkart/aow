import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  async function fixture(t, options = {}) {
    const page = await browser.newPage();
    const errors = []; page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    const session = { thread_id: 'thread', session_id: 'remote', agent_id: 'test', cwd: '/repo', title: 'Test', status: 'idle', sync_generation: 'g1', revision: 1, overview: [], entries: [], permissions: [], modes: {}, config_options: [], commands: [], usage: null };
    const state = { session, peers: new Set(), counts: {}, subscriptions: 0, sockets: 0, boot: 'b1', version: 1, active: 0, maxActive: 0, gate: undefined, deleted: false, supported: true, ...options };
    const head = resource => ({ resource, boot_id: state.boot, version: state.version, deleted: state.deleted, ...(resource.kind === 'session' ? { session: { generation: session.sync_generation, revision: session.revision, info: session } } : {}) });
    await page.routeWebSocket('**/api/events/ws', socket => {
      state.sockets++;
      const peer = { socket, subscriptions: new Map(), send: data => socket.send(JSON.stringify({ event: 'acp', data })) };
      state.peers.add(peer);
      if (state.supported) peer.send({ type: 'hello', protocol: 1 });
      socket.onMessage(raw => {
        const command = JSON.parse(String(raw));
        if (command.action === 'unsubscribe') { peer.subscriptions.delete(command.id); return; }
        state.subscriptions++; peer.subscriptions.set(command.id, command.resource);
        peer.send({ type: 'ready', id: command.id, head: head(command.resource) });
      });
      socket.onClose(() => state.peers.delete(peer));
    });
    state.emit = (kind = 'session') => {
      for (const peer of state.peers) for (const [id, resource] of peer.subscriptions) if (resource.kind === kind) peer.send({ type: 'change', ids: [id], head: head(resource) });
    };
    await page.route('**/api/acp/**', async route => {
      const url = new URL(route.request().url());
      const key = url.pathname.split('/').at(-1);
      state.counts[key] = (state.counts[key] ?? 0) + 1;
      if (state.failNext) { state.failNext = false; await route.fulfill({ status: 503, json: { message: 'Temporary failure' } }); return; }
      state.active++; state.maxActive = Math.max(state.maxActive, state.active);
      // Capture the response before waiting, so notifications can overtake it.
      const snapshot = structuredClone(session);
      state.onCaptured?.();
      if (state.gate && (key === 'thread' || key === 'updates')) await state.gate;
      const json = key === 'requests' ? [] : key === 'connection' ? { id: 'conn', agent_id: 'test', cwd: '/repo', auth_methods: [], capabilities: {}, prompt_capabilities: {} }
        : key === 'sessions' ? [snapshot] : key === 'updates' ? { kind: 'snapshot', snapshot } : snapshot;
      state.active--;
      await route.fulfill({ json }).catch(() => {});
    });
    await page.route('**/sync-test', route => route.fulfill({ contentType: 'text/html', body: '<!doctype html><html lang="en"><title>ACP sync test</title><body></body></html>' }));
    await page.goto(`${base}/sync-test`);
    await page.evaluate(async () => {
      window.syncModule = await import('/src/features/acp/sync/store.ts');
      window.readers = new Map();
      window.mountResource = (id, resource, read = true) => {
        const store = window.syncModule.getResource(resource);
        const stop = store.subscribe(() => {}, read);
        window.readers.set(id, { store, stop });
      };
    });
    return { page, state };
  }
  const mount = (page, id, resource, read = true) => page.evaluate(({ id, resource, read }) => window.mountResource(id, resource, read), { id, resource, read });
  const ready = (page, id, revision = 1) => page.waitForFunction(({ id, revision }) => window.readers.get(id)?.store.state.data?.revision === revision, { id, revision });
  const sessionResource = { kind: 'session', thread_id: 'thread' };

  await test('healthy idle resources make zero periodic business reads over 60 seconds and share subscriptions', async t => {
    const { page, state } = await fixture(t);
    await page.clock.install();
    await mount(page, 'thread', sessionResource);
    await mount(page, 'duplicate', sessionResource);
    await mount(page, 'binding', { kind: 'connection', thread_id: 'thread' });
    await mount(page, 'requests', { kind: 'requests', connection_id: 'conn' });
    await mount(page, 'authentication', { kind: 'requests', connection_id: 'conn' });
    await mount(page, 'list', { kind: 'sessions', cwd: '/repo' });
    await ready(page, 'thread');
    await page.waitForFunction(() => [...window.readers.values()].every(({ store }) => store.state.data !== undefined));
    const counts = { ...state.counts };
    assert.equal(state.sockets, 1); assert.equal(state.subscriptions, 4);
    assert.equal(counts.requests, 1); assert.equal(counts.connection, 1); assert.equal(counts.sessions, 1);
    await page.clock.runFor(60_000);
    assert.deepEqual(state.counts, counts);
    for (let i = 0; i < 100; i++) state.emit();
    await page.clock.runFor(1000);
    assert.deepEqual(state.counts, counts, 'duplicate notifications never amplify reads');
  });

  await test('snapshot race catches up once, ignores old notifications, and never overlaps a resource read', async t => {
    const { page, state } = await fixture(t);
    let release; state.gate = new Promise(resolve => { release = resolve; });
    const captured = new Promise(resolve => { state.onCaptured = resolve; });
    await mount(page, 'thread', sessionResource);
    await captured;
    state.session.revision = 8; state.version = 8;
    for (let i = 0; i < 40; i++) state.emit();
    state.gate = undefined; release();
    await ready(page, 'thread', 8);
    assert.equal(state.counts.thread, 1); assert.equal(state.counts.updates, 1); assert.equal(state.maxActive, 1);
    state.version = 4; state.session.revision = 4; state.emit();
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(resolve)));
    assert.equal(await page.evaluate(() => window.readers.get('thread').store.state.data.revision), 8);
  });

  await test('hidden views track metadata without body reads and reuse cache across StrictMode and portal remounts', async t => {
    const { page, state } = await fixture(t);
    await mount(page, 'thread', sessionResource);
    await ready(page, 'thread');
    await page.evaluate(resource => {
      window.readers.get('thread').stop();
      window.mountResource('thread', resource, false);
    }, sessionResource);
    state.session.revision = 2; state.version = 2; state.session.status = 'working'; state.emit();
    await page.waitForFunction(() => window.readers.get('thread').store.state.head?.session.info.status === 'working');
    assert.equal(state.counts.updates ?? 0, 0);
    await page.evaluate(resource => {
      window.readers.get('thread').stop(); window.mountResource('thread', resource);
    }, sessionResource);
    await ready(page, 'thread', 2);
    assert.equal(state.counts.updates, 1); assert.equal(state.subscriptions, 1); assert.equal(state.sockets, 1);
  });

  await test('reconnect revalidates versions, boot changes replace snapshots, and deletion terminates reads', async t => {
    const { page, state } = await fixture(t);
    await mount(page, 'thread', sessionResource);
    await ready(page, 'thread');
    state.session.revision = 5; state.version = 5;
    for (const peer of state.peers) peer.socket.close();
    await ready(page, 'thread', 5);
    assert.equal(state.sockets, 2);
    state.boot = 'b2'; state.version = 0; state.session.sync_generation = 'g2'; state.session.revision = 1;
    for (const peer of state.peers) peer.socket.close();
    await page.waitForFunction(() => window.readers.get('thread').store.state.data?.sync_generation === 'g2');
    assert.equal(state.counts.thread, 2, 'new generations use snapshots');
    state.deleted = true; state.version = 1; state.emit();
    await page.waitForFunction(() => window.readers.get('thread').store.state.deleted);
    const counts = { ...state.counts };
    await page.clock.install(); await page.clock.runFor(60_000);
    assert.deepEqual(state.counts, counts);
    assert.equal(await page.evaluate(() => window.readers.get('thread').store.state.data), undefined);
  });
  await test('legacy capability fallback is shared and stops immediately when push becomes available', async t => {
    const { page, state } = await fixture(t, { supported: false });
    await page.clock.install();
    await mount(page, 'thread', sessionResource);
    await mount(page, 'duplicate', sessionResource);
    await page.clock.runFor(2100);
    await ready(page, 'thread');
    assert.equal(await page.evaluate(() => window.readers.get('thread').store.state.status), 'degraded');
    const initial = state.counts.thread;
    await page.clock.runFor(30_000);
    await page.waitForFunction(() => !window.readers.get('thread').store.state.loading);
    assert.equal(state.counts.updates, 1);
    for (const peer of state.peers) peer.send({ type: 'hello', protocol: 1 });
    await page.waitForFunction(() => window.readers.get('thread').store.state.status === 'live');
    await page.clock.runFor(60_000);
    assert.equal(state.counts.thread, initial); assert.equal(state.counts.updates, 1);
    assert.equal(state.subscriptions, 1);
  });

  await test('generation changes discard an older in-flight response and authentication revocation stops retries', async t => {
    const { page, state } = await fixture(t);
    await mount(page, 'thread', sessionResource); await ready(page, 'thread');
    let release; state.gate = new Promise(resolve => { release = resolve; });
    const captured = new Promise(resolve => { state.onCaptured = resolve; });
    state.session.revision = 2; state.version = 2; state.emit();
    await captured;
    state.session.sync_generation = 'g2'; state.session.revision = 1; state.version = 3; state.emit();
    state.gate = undefined; release();
    await page.waitForFunction(() => window.readers.get('thread').store.state.data?.sync_generation === 'g2');
    const counts = { ...state.counts };
    for (const peer of state.peers) peer.socket.close({ code: 1008, reason: 'Authentication expired' });
    await page.waitForFunction(() => window.readers.get('thread').store.state.status === 'denied');
    await page.clock.install(); await page.clock.runFor(60_000);
    assert.deepEqual(state.counts, counts); assert.equal(state.sockets, 1);
    assert.equal(await page.evaluate(() => window.readers.get('thread').store.state.data), undefined);
  });
  await test('an explicit shared read recovers a failed request and cancels its pending retry timer', async t => {
    const { page, state } = await fixture(t);
    await page.clock.install(); state.failNext = true;
    await mount(page, 'thread', sessionResource);
    await page.waitForFunction(() => window.readers.get('thread').store.state.error === 'Temporary failure');
    const recovered = await page.evaluate(() => window.syncModule.readSession('thread'));
    assert.equal(recovered.revision, 1);
    const counts = { ...state.counts };
    await page.clock.runFor(60_000);
    assert.deepEqual(state.counts, counts);
    assert.equal(await page.evaluate(() => window.readers.get('thread').store.state.error), '');
  });
} finally { await browser?.close(); await server.close(); }
