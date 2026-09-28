import { appUrl } from './basePath';

type EventName = 'workspace' | 'operations' | 'task-stopped';
const subscribers = new Set<LiveEvents>();
const snapshots = new Map<EventName, string>();
let socket: WebSocket | undefined;
let retry: ReturnType<typeof setTimeout> | undefined;
let connecting: ReturnType<typeof setTimeout> | undefined;
let attempts = 0;
let denied = false;

function connect() {
  if (socket || !subscribers.size || denied) return;
  clearTimeout(retry);
  retry = undefined;
  const url = new URL(appUrl('/api/events/ws'), window.location.href);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  let current: WebSocket;
  try { current = new WebSocket(url); }
  catch { disconnected(); return; }
  socket = current;
  connecting = setTimeout(() => current.close(), 10_000);
  current.onopen = () => {
    if (socket !== current) return;
    clearTimeout(connecting);
    attempts = 0;
    for (const listener of subscribers) listener.open();
  };
  current.onmessage = event => {
    if (socket !== current || typeof event.data !== 'string') return;
    let envelope: { event?: EventName; data?: unknown };
    try { envelope = JSON.parse(event.data); } catch { return; }
    if (!envelope || !['workspace', 'operations', 'task-stopped'].includes(envelope.event ?? '') || envelope.data === undefined) return;
    const name = envelope.event!;
    const data = JSON.stringify(envelope.data);
    // State snapshots may be replayed to late subscribers; notifications never are.
    if (name !== 'task-stopped') snapshots.set(name, data);
    for (const listener of subscribers) if (listener.name === name) listener.receive(data);
  };
  current.onerror = () => current.close();
  current.onclose = event => {
    if (socket !== current) return;
    socket = undefined;
    denied = event.code === 1008;
    disconnected();
  };
}

function disconnected() {
  clearTimeout(connecting);
  snapshots.clear();
  for (const listener of subscribers) listener.failed();
  if (subscribers.size && !denied) {
    retry = setTimeout(connect, Math.min(30_000, 1000 * 2 ** Math.min(attempts++, 5)));
  }
}

function resume() {
  if (document.visibilityState === 'visible') connect();
}

function reconcile() {
  if (subscribers.size) { connect(); return; }
  clearTimeout(retry);
  clearTimeout(connecting);
  const previous = socket;
  socket = undefined;
  previous?.close();
  snapshots.clear();
  attempts = 0;
  denied = false;
  window.removeEventListener('online', resume);
  document.removeEventListener('visibilitychange', resume);
}

/** EventSource-like subscriptions sharing one WebSocket per page, outside the HTTP pool. */
export class LiveEvents extends EventTarget {
  static readonly OPEN = 1;
  readyState = 0;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  private closed = false;

  constructor(readonly name: EventName) {
    super();
    if (!subscribers.size) {
      window.addEventListener('online', resume);
      document.addEventListener('visibilitychange', resume);
    }
    subscribers.add(this);
    queueMicrotask(() => {
      if (this.closed) return;
      if (socket?.readyState === WebSocket.OPEN) {
        this.open();
        const cached = snapshots.get(name);
        if (cached !== undefined) this.receive(cached);
      } else { connect(); }
    });
  }

  open() {
    if (this.closed) return;
    this.readyState = LiveEvents.OPEN;
    this.onopen?.();
  }

  receive(data: string) {
    if (!this.closed) this.dispatchEvent(new MessageEvent(this.name, { data }));
  }

  failed() {
    if (this.closed) return;
    this.readyState = denied ? 2 : 0;
    this.onerror?.();
  }

  close() {
    this.closed = true;
    this.readyState = 2;
    subscribers.delete(this);
    // React StrictMode and section switches may replace subscriptions in one turn.
    queueMicrotask(reconcile);
  }
}
