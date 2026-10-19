// A protocol peer for component tests. Observable fixture mutations represent
// committed server changes; no browser polling or test-side polling is needed.
const observers = new Set();
const connections = new WeakMap();
export function disconnectAcpEvents(page) {
  for (const peer of connections.get(page) ?? []) peer.socket.close();
}
export async function acpEventsFixture(page, initial) {
  const peers = new Set();
  connections.set(page, peers);
  const latest = new Map();
  const proxies = new WeakMap();
  let version = 0;
  let scheduled = false;
  const info = value => value && Object.fromEntries(['thread_id', 'parent', 'source', 'created_at', 'session_id', 'agent_id', 'cwd', 'title', 'status', 'updated_at', 'interacted_at'].map(key => [key, value[key]]));
  function observe(value) {
    if (!value || typeof value !== 'object' || ![Object.prototype, Array.prototype].includes(Object.getPrototypeOf(value))) return value;
    if (proxies.has(value)) return proxies.get(value);
    const proxy = new Proxy(value, {
      get(target, key, receiver) { const next = Reflect.get(target, key, receiver); return key === 'controls' ? next : observe(next); },
      set(target, key, next, receiver) { const result = Reflect.set(target, key, next, receiver); for (const observer of observers) observer(); return result; },
      deleteProperty(target, key) { const result = Reflect.deleteProperty(target, key); for (const observer of observers) observer(); return result; },
    });
    proxies.set(value, proxy); return proxy;
  }
  const state = observe(initial);
  const metadata = () => JSON.stringify(state.sessions?.map(session =>
    [session.thread_id, session.parent, session.agent_id, session.cwd, session.title]));
  let previousMetadata = metadata();
  let instances = 0;
  const workspace = () => ({ boot_id: 'fixture', revision: instances, projects: 0, terminals: 0, instances, repositories: {} });
  await page.route('**/api/instances', route => route.fulfill({ json: { items: state.instances ?? [] } }));
  observers.add(schedule);
  page.on('close', () => observers.delete(schedule));
  function head(resource) {
    const session = state.sessions?.find(item => item.thread_id === resource.thread_id);
    let signature;
    let deleted = false;
    switch (resource.kind) {
      case 'session': signature = [session, state.updatePayload]; deleted = !session; break;
      case 'sessions': signature = state.sessions?.filter(item => item.cwd === resource.cwd).map(info); break;
      case 'connection': signature = [state.authRequired, state.advertiseAuth, state.imageSupport, state.capabilities, state.extensions, !!session]; deleted = !session; break;
      case 'requests': signature = state.authenticating ? state.authRequests : state.connectionRequests; break;
      case 'operation': signature = resource.cwd === null ? state.installStatus : state.connectionStatus; break;
    }
    const key = JSON.stringify(resource);
    const encoded = JSON.stringify(signature);
    const previous = latest.get(key);
    if (!previous || previous.encoded !== encoded) {
      const event = { resource, boot_id: 'fixture', version: ++version, deleted };
      if (resource.kind === 'session' && session?.sync_generation) event.session = { generation: session.sync_generation, revision: Math.max(session.revision, state.updatePayload?.snapshot?.revision ?? state.updatePayload?.revision ?? 0), info: info(session) };
      latest.set(key, { encoded, event });
    }
    return latest.get(key).event;
  }
  function schedule() {
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      const nextMetadata = metadata();
      if (nextMetadata !== previousMetadata) {
        previousMetadata = nextMetadata;
        instances++;
        for (const peer of peers) peer.socket.send(JSON.stringify({ event: 'workspace', data: workspace() }));
      }
      for (const peer of peers) for (const [id, subscription] of peer.subscriptions) {
        const next = head(subscription.resource);
        if (next.version <= subscription.version) continue;
        subscription.version = next.version;
        peer.send({ type: 'change', ids: [id], head: next });
        if (next.deleted) peer.subscriptions.delete(id);
      }
    });
  }
  await page.routeWebSocket('**/api/events/ws', socket => {
    const peer = { socket, subscriptions: new Map(), send: data => socket.send(JSON.stringify({ event: 'acp', data })) };
    peers.add(peer);
    socket.send(JSON.stringify({ event: 'workspace', data: workspace() }));
    peer.send({ type: 'hello', protocol: 1 });
    socket.onMessage(raw => {
      const command = JSON.parse(String(raw));
      if (command.action === 'unsubscribe') { peer.subscriptions.delete(command.id); return; }
      const next = head(command.resource);
      if (!next.deleted) peer.subscriptions.set(command.id, { resource: command.resource, version: next.version });
      peer.send({ type: 'ready', id: command.id, head: next });
    });
    socket.onClose(() => peers.delete(peer));
  });
  return state;
}
