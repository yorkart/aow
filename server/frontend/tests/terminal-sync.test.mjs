import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright';

const server = await createServer({ root: fileURLToPath(new URL('../', import.meta.url)), logLevel: 'error', server: { host: '127.0.0.1', port: 0, proxy: {} } });
const tab = (id, root = '/one') => ({ id, name: id, workspace_root: root, revision: 1,
  layout: { type: 'pane', pane_id: `${id}-pane` }, panes: [{ id: `${id}-pane`, name: 'shell', cwd: root, shell: '/bin/sh', status: 'running' }] });
const empty = () => ({ agents: {}, titles: {}, processes: {}, activity: {} });
const until = async check => {
  for (let i = 0; i < 200; i++) { if (await check()) return; await delay(20); }
  assert.fail('Timed out waiting for terminal synchronization');
};
let browser;
try {
  await server.listen();
  browser = await chromium.launch({ headless: true, args: ['--no-sandbox'], ...(process.env.CHROMIUM_PATH ? { executablePath: process.env.CHROMIUM_PATH } : {}) });
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  async function fixture(t, options = {}) {
    const page = await browser.newPage();
    await page.addInitScript(() => { try { localStorage.setItem('aow.language', 'zh-CN'); } catch { /* Explicit Chinese fixture with blocked-storage coverage. */ } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    t.after(async () => { await page.close(); assert.deepEqual(errors, []); });
    const state = { tabs: [tab('one'), tab('two', '/two')], metadata: empty(), boot: 'boot', revision: 1, error: null,
      peers: new Set(), requests: [], subscriptions: 0, unsubscriptions: 0, supported: true, ...options };
    state.snapshot = () => ({ boot_id: state.boot, revision: state.revision, tabs: state.tabs, metadata: state.metadata, error: state.error, agent_error: null });
    await page.routeWebSocket('**/api/events/ws*', socket => {
      const peer = { socket, active: false, send: data => socket.send(JSON.stringify({ event: 'terminals', data })) };
      state.peers.add(peer);
      if (state.supported) peer.send({ type: 'hello', protocol: 1 });
      socket.onMessage(raw => {
        const command = JSON.parse(String(raw));
        if (command.action === 'subscribe_terminals') {
          peer.active = true; state.subscriptions++;
          peer.send({ type: 'snapshot', state: state.snapshot() });
        } else if (command.action === 'unsubscribe_terminals') {
          peer.active = false; state.unsubscriptions++;
        } else if (command.action === 'refresh_terminals') {
          const reply = () => peer.send({ type: 'snapshot', state: state.snapshot(), request_id: command.id });
          if (state.holdRefresh) state.reply = reply;
          else reply();
        }
      });
      socket.onClose(() => state.peers.delete(peer));
    });
    state.emit = () => {
      for (const peer of state.peers) if (peer.active) peer.send({ type: 'snapshot', state: state.snapshot() });
    };
    await page.route('**/api/terminals**', async route => {
      state.requests.push(route.request().url());
      const request = route.request();
      if (request.method() !== 'GET') {
        await state.mutationGate;
        if (state.failure) { await route.fulfill({ status: state.failure, json: { message: 'mutation failed' } }); return; }
        if (request.method() === 'POST') {
          const created = tab('created', request.postDataJSON().workspace_root);
          state.tabs.push(created); state.revision++;
          await route.fulfill({ json: created }); return;
        }
        const id = new URL(request.url()).pathname.split('/').at(-1);
        if (request.method() === 'DELETE') {
          state.tabs = state.tabs.filter(tab => tab.id !== id); state.revision++;
          await route.fulfill({ status: 204 }); return;
        }
        if (request.method() === 'PATCH') {
          const updated = state.tabs.find(tab => tab.id === id);
          Object.assign(updated, request.postDataJSON(), { revision: updated.revision + 1 }); state.revision++;
          await route.fulfill({ json: updated }); return;
        }
      }
      const json = structuredClone(new URL(route.request().url()).pathname.endsWith('/agents') ? state.metadata : state.tabs);
      await state.gate;
      await route.fulfill(state.failure ? { status: state.failure, json: { message: 'unavailable' } } : { json }).catch(() => {});
    });
    await page.route('**/terminal-sync-test', async route => route.fulfill({ contentType: 'text/html', body: await server.transformIndexHtml('/terminal-sync-test', `<!doctype html><html lang="en"><title>Terminal sync test</title><body>${state.hooks ? '<script type="module" src="/tests/terminal-sync-preview.tsx"></script>' : ''}</body></html>`) }));
    await page.goto(`${base}/terminal-sync-test`);
    await page.evaluate(async () => {
      window.store = (await import('/src/features/terminals/terminalStore.ts')).terminalStore;
      window.readers = new Map();
      window.mount = id => window.readers.set(id, window.store.subscribe(() => {}));
      window.unmount = id => { window.readers.get(id)?.(); window.readers.delete(id); };
    });
    return { page, state };
  }
  const mount = page => page.evaluate(() => window.mount('main'));
  const ready = page => page.waitForFunction(() => window.store.snapshot().tabs?.length === 2);

  await test('workspace hooks and the project sidebar share updates, mutations and errors without polling', async t => {
    const { page, state } = await fixture(t, { hooks: true });
    await page.waitForFunction(() => window.terminalProbes?.['/one']?.loaded && window.terminalProbes?.['/two']?.loaded && window.projectProbe?.loaded);
    assert.equal(state.subscriptions, 1);
    assert.deepEqual(state.requests, []);
    assert.deepEqual(await page.evaluate(() => Object.values(window.terminalProbes).map(probe => probe.tabs.map(tab => tab.id))), [['one'], ['two']]);
    await page.evaluate(() => window.terminalProbes['/one'].rename('one', 'renamed'));
    await page.waitForFunction(() => window.projectProbe.tabs[0].name === 'renamed');
    state.failure = 503;
    await page.evaluate(() => window.terminalProbes['/one'].rename('one', 'failed'));
    await page.waitForFunction(() => window.terminalProbes['/one'].error === 'mutation failed');
    await delay(100);
    assert.equal(await page.evaluate(() => window.terminalProbes['/one'].error), 'mutation failed');
    state.failure = undefined;
    await page.evaluate(() => window.terminalProbes['/one'].close('one'));
    await page.waitForFunction(() => window.terminalProbes['/one'].tabs.length === 0 && window.projectProbe.tabs.length === 1);
    const requests = state.requests.length;
    await page.clock.install(); await page.clock.runFor(60_000);
    assert.equal(state.requests.length, requests);
    assert.deepEqual(await page.evaluate(() => window.terminalProbes['/two'].tabs.map(tab => tab.id)), ['two']);
  });

  for (const failure of [undefined, 503]) {
    await test(`authentication revocation discards delayed mutation ${failure ? 'failures' : 'successes'} and old callbacks`, async t => {
      const { page, state } = await fixture(t, { hooks: true, failure });
      await page.waitForFunction(() => window.terminalProbes?.['/one']?.loaded);
      let release;
      state.mutationGate = new Promise(resolve => { release = resolve; });
      t.after(() => release());
      await page.evaluate(() => {
        window.oldProbe = window.terminalProbes['/one'];
        window.oldTab = structuredClone(window.oldProbe.tabs[0]);
        window.delayedCreate = window.oldProbe.create();
      });
      await until(() => state.requests.length === 1);
      for (const peer of [...state.peers]) peer.socket.close({ code: 1008, reason: 'expired' });
      await page.waitForFunction(() => window.store.snapshot().status === 'denied'
        && !window.terminalProbes['/one'].tabs.length && !window.terminalProbes['/one'].busy);
      release();
      assert.equal(await page.evaluate(() => window.delayedCreate), null);
      await page.evaluate(() => {
        // Layout/rebuild responses use callbacks captured before revocation.
        window.oldProbe.replace(window.oldTab);
        window.terminalProbes['/one'].replace(window.oldTab);
        window.oldProbe.updatePaneStatus(window.oldTab.id, window.oldTab.panes[0].id, 'exited');
        void window.oldProbe.rename(window.oldTab.id, 'stale');
      });
      await delay(100);
      assert.deepEqual(await page.evaluate(() => ({
        status: window.store.snapshot().status,
        shared: window.store.snapshot().tabs,
        local: window.terminalProbes['/one'].tabs,
        error: window.terminalProbes['/one'].error,
        busy: window.terminalProbes['/one'].busy,
      })), { status: 'denied', shared: undefined, local: [], error: '登录已失效', busy: false });
      assert.equal(state.requests.length, 1, 'invalidated callbacks must not initiate more requests');
      // Signing in again mounts a fresh workspace in the same page. Old
      // callbacks must stay invalid even though the new store is live.
      await page.evaluate(() => window.setTerminalPreviewMounted(false));
      await page.waitForFunction(() => window.store.snapshot().status === 'connecting');
      state.tabs = [tab('new-session')]; state.revision++;
      await page.evaluate(() => window.setTerminalPreviewMounted(true));
      await page.waitForFunction(() => window.terminalProbes['/one'].tabs[0]?.id === 'new-session');
      await page.evaluate(() => {
        window.oldProbe.replace(window.oldTab);
        void window.oldProbe.close('new-session');
      });
      await delay(100);
      assert.deepEqual(await page.evaluate(() => window.store.snapshot().tabs.map(tab => tab.id)), ['new-session']);
      assert.equal(state.requests.length, 1, 'an old session must not mutate the new session');
    });
  }

  await test('many terminal consumers share one subscription and make zero HTTP reads over an idle minute', async t => {
    const { page, state } = await fixture(t);
    await page.clock.install();
    await page.evaluate(() => { for (let i = 0; i < 20; i++) window.mount(String(i)); });
    await ready(page);
    await page.clock.runFor(60_000);
    assert.equal(state.peers.size, 1);
    assert.equal(state.subscriptions, 1);
    assert.deepEqual(state.requests, []);
    state.tabs = [tab('changed', '/two')]; state.metadata = { ...empty(), agents: { 'changed-pane': 'codex' }, titles: { 'changed-pane': 'New title' } }; state.revision++;
    state.emit();
    await page.waitForFunction(() => window.store.snapshot().tabs?.[0].id === 'changed');
    assert.equal(await page.evaluate(() => window.store.snapshot().metadata.titles['changed-pane']), 'New title');
    assert.deepEqual(state.requests, []);
    await page.evaluate(() => { for (const id of [...window.readers.keys()]) window.unmount(id); });
    await until(() => state.unsubscriptions === 1);
  });

  await test('snapshot ordering and server restart replace the full state, including deletions', async t => {
    const { page, state } = await fixture(t);
    await mount(page); await ready(page);
    state.tabs = [tab('new')]; state.revision = 10; state.emit();
    await page.waitForFunction(() => window.store.snapshot().tabs?.[0].id === 'new');
    state.tabs = [tab('stale')]; state.revision = 9; state.emit(); await delay(50);
    assert.equal(await page.evaluate(() => window.store.snapshot().tabs[0].id), 'new');
    state.boot = 'restarted'; state.revision = 1; state.tabs = []; state.emit();
    await page.waitForFunction(() => window.store.snapshot().tabs?.length === 0);
  });

  await test('committed local mutations are protected against snapshots queued before acknowledgement', async t => {
    const { page, state } = await fixture(t, { holdRefresh: true });
    await mount(page); await ready(page);
    await page.evaluate(() => window.store.remove('one'));
    await until(() => !!state.reply);
    state.revision++; state.emit(); await delay(50);
    assert.deepEqual(await page.evaluate(() => window.store.snapshot().tabs.map(tab => tab.id)), ['two']);
    state.tabs = [tab('two', '/two')]; state.revision++; state.reply();
    await delay(50);
    state.tabs = []; state.revision++; state.emit();
    await page.waitForFunction(() => window.store.snapshot().tabs?.length === 0);
  });

  await test('hidden pages unsubscribe and receive a fresh snapshot when visible again', async t => {
    const { page, state } = await fixture(t);
    await page.clock.install(); await mount(page); await ready(page);
    await page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'hidden' });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await until(() => state.unsubscriptions === 1);
    state.tabs = []; state.revision++;
    await page.clock.runFor(60_000);
    assert.deepEqual(state.requests, []);
    await page.evaluate(() => {
      Object.defineProperty(document, 'visibilityState', { configurable: true, value: 'visible' });
      document.dispatchEvent(new Event('visibilitychange'));
    });
    await page.waitForFunction(() => window.store.snapshot().tabs?.length === 0);
    assert.equal(state.subscriptions, 2);
  });

  await test('older servers use one shared low-frequency fallback and preserve data during failure', async t => {
    const { page, state } = await fixture(t, { supported: false, metadata: { ...empty(), agents: { 'one-pane': 'codex' } } });
    await page.clock.install(); await mount(page); await page.evaluate(() => window.mount('second'));
    await page.clock.runFor(2100); await ready(page);
    assert.equal(state.requests.length, 2);
    state.failure = 503;
    await page.clock.runFor(30_000); await until(() => state.requests.length === 4);
    await page.waitForFunction(() => window.store.snapshot().error !== '');
    assert.equal(await page.evaluate(() => window.store.snapshot().metadata.agents['one-pane']), 'codex');
    await page.clock.runFor(29_000); assert.equal(state.requests.length, 4);
    state.failure = undefined; state.tabs = [];
    await page.clock.runFor(1000);
    await page.waitForFunction(() => window.store.snapshot().tabs?.length === 0 && !window.store.snapshot().error);
  });

  await test('a pushed snapshot supersedes an in-flight fallback response', async t => {
    const { page, state } = await fixture(t, { supported: false });
    let release; state.gate = new Promise(resolve => { release = resolve; });
    await page.clock.install(); await mount(page); await page.clock.runFor(2100);
    await until(() => state.requests.length === 2);
    state.tabs = [tab('pushed')]; state.revision = 2;
    for (const peer of state.peers) peer.send({ type: 'hello', protocol: 1 });
    await page.waitForFunction(() => window.store.snapshot().tabs?.[0].id === 'pushed');
    release(); await delay(100);
    assert.equal(await page.evaluate(() => window.store.snapshot().tabs[0].id), 'pushed');
    await page.clock.runFor(60_000);
    assert.equal(state.requests.length, 2);
  });

  await test('reconnect resubscribes and authentication revocation clears data without polling', async t => {
    const { page, state } = await fixture(t, { hooks: true });
    await page.clock.install(); await mount(page); await ready(page);
    state.tabs = [tab('reconnected')]; state.revision++;
    for (const peer of [...state.peers]) peer.socket.close();
    await until(async () => await page.evaluate(() => window.store.snapshot().status) === 'offline');
    await page.clock.runFor(1100);
    await page.waitForFunction(() => window.store.snapshot().status === 'live' && window.store.snapshot().tabs?.[0].id === 'reconnected');
    assert.equal(state.subscriptions, 2);
    for (const peer of [...state.peers]) peer.socket.close({ code: 1008, reason: 'expired' });
    await page.waitForFunction(() => window.store.snapshot().status === 'denied');
    await page.waitForFunction(() => Object.values(window.terminalProbes).every(probe => probe.tabs.length === 0 && !probe.loaded));
    const requests = state.requests.length;
    await page.clock.runFor(60_000);
    assert.equal(state.requests.length, requests);
    assert.equal(await page.evaluate(() => window.store.snapshot().tabs), undefined);
  });
} finally {
  await browser?.close();
  await server.close();
}
