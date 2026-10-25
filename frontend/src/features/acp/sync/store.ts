import { t } from '../../../lib/i18n';
import { LiveEvents } from '../../../lib/liveEvents';
import { acpApi } from '../api';
import { AcpRequestError } from '../request';
import { applySessionUpdates } from '../session_updates';
import type { ConnectionInfo, ConnectionStatus, Permission, SessionInfo, SessionSnapshot } from '../types';

export type Resource = { kind: 'session' | 'connection'; thread_id: string }
  | { kind: 'sessions'; cwd: string }
  | { kind: 'requests'; connection_id: string }
  | { kind: 'operation'; agent_id: string; cwd: string | null };
export type SyncStatus = 'connecting' | 'live' | 'offline' | 'degraded' | 'denied';
export interface Head {
  resource: Resource; boot_id: string; version: number; deleted: boolean;
  session?: { generation: string; revision: number; info: SessionInfo };
}
type Data = SessionSnapshot | SessionInfo[] | ConnectionInfo | Permission[] | ConnectionStatus | null;
export interface SyncState<T = Data> {
  data?: T; head?: Head; error: string; deleted: boolean; status: SyncStatus; loading: boolean;
}
const stores = new Map<string, ResourceStore>();
let source: LiveEvents | undefined;
let status: SyncStatus = 'connecting';
let helloTimer: ReturnType<typeof setTimeout> | undefined;
let fallback: ReturnType<typeof setInterval> | undefined;
let protocol = false;
let serial = 0;

export function resourceKey(resource: Resource) {
  switch (resource.kind) {
    case 'session': case 'connection': return JSON.stringify([resource.kind, resource.thread_id]);
    case 'sessions': return JSON.stringify([resource.kind, resource.cwd]);
    case 'requests': return JSON.stringify([resource.kind, resource.connection_id]);
    case 'operation': return JSON.stringify([resource.kind, resource.agent_id, resource.cwd]);
  }
}

function setStatus(next: SyncStatus) {
  status = next;
  for (const store of stores.values()) if (store.active) store.publish({ status: next });
}
function stopFallback() { clearInterval(fallback); fallback = undefined; clearTimeout(helloTimer); }
function degrade() {
  if (!source || protocol || source.readyState !== LiveEvents.OPEN) return;
  setStatus('degraded');
  for (const store of stores.values()) if (store.active) { store.ready = true; store.revalidate(); }
  if (!fallback) fallback = setInterval(() => {
    for (const store of stores.values()) if (store.active) store.revalidate();
  }, 30_000);
}
function connect() {
  if (source) return;
  const connection = new LiveEvents('acp');
  source = connection;
  connection.onopen = () => {
    protocol = false; stopFallback(); setStatus('connecting');
    // Only an open server without the capability greeting enters compatibility mode.
    helloTimer = setTimeout(degrade, 2000);
  };
  connection.onerror = () => {
    protocol = false; stopFallback();
    const denied = connection.readyState === 2;
    setStatus(denied ? 'denied' : 'offline');
    for (const store of stores.values()) store.disconnected(denied);
  };
  connection.addEventListener('acp', event => {
    if (source !== connection) return;
    let message: { type: string; protocol?: number; id?: string; ids?: string[]; head?: Head };
    try { message = JSON.parse((event as MessageEvent<string>).data); } catch { return; }
    if (message.type === 'hello' && message.protocol === 1) {
      if (protocol) return;
      protocol = true; stopFallback(); setStatus('live');
      for (const store of stores.values()) if (store.active) store.start();
    } else if (message.type === 'resync') {
      connection.reconnect();
    } else if (protocol && (message.type === 'ready' || message.type === 'change') && message.head) {
      const head = message.head;
      if (typeof head.boot_id !== 'string' || !Number.isSafeInteger(head.version) || !head.resource) return;
      const store = stores.get(resourceKey(head.resource));
      const addressed = message.type === 'ready' ? message.id === store?.id : message.ids?.includes(store?.id ?? '');
      if (store?.active && addressed) store.receive(head, message.type === 'ready');
    }
  });
}
function reconcile() {
  if ([...stores.values()].some(store => store.active)) return;
  source?.close(); source = undefined; protocol = false; stopFallback(); status = 'connecting';
}

export class ResourceStore {
  readonly key: string;
  id = '';
  ready = false;
  state: SyncState = { error: '', deleted: false, status, loading: true };
  private listeners = new Map<() => void, boolean>();
  private pending?: Promise<void>;
  private controller?: AbortController;
  private retry?: ReturnType<typeof setTimeout>;
  private eviction?: ReturnType<typeof setTimeout>;
  private epoch = 0;
  private dirty = true;
  private forced = false;
  private failures = 0;
  constructor(readonly resource: Resource) {
    this.key = resourceKey(resource);
    // An abandoned concurrent render may never subscribe to its allocated store.
    this.eviction = setTimeout(() => { if (!this.active && stores.get(this.key) === this) stores.delete(this.key); }, 30_000);
  }
  get active() { return this.listeners.size > 0; }
  private get reading() { return [...this.listeners.values()].some(Boolean); }
  snapshot = () => this.state;
  publish(change: Partial<SyncState>) {
    this.state = { ...this.state, ...change };
    for (const listener of this.listeners.keys()) listener();
  }
  subscribe = (listener: () => void, read = true) => {
    clearTimeout(this.eviction);
    this.listeners.set(listener, read);
    this.publish({ status });
    connect();
    if (!this.id && protocol) this.start();
    if (status === 'degraded') { this.ready = true; this.revalidate(); }
    else void this.read();
    return () => {
      this.listeners.delete(listener);
      queueMicrotask(() => {
        if (this.active) { void this.read(); return; }
        this.unsubscribe(); clearTimeout(this.retry); this.retry = undefined;
        // Keep a bounded-duration cache and in-flight read across portal moves.
        clearTimeout(this.eviction);
        this.eviction = setTimeout(() => {
          if (this.active) return;
          this.controller?.abort();
          if (stores.get(this.key) === this) stores.delete(this.key);
        }, 30_000);
        reconcile();
      });
    };
  };
  private unsubscribe() {
    if (this.id) source?.send({ action: 'unsubscribe', id: this.id });
    this.id = ''; this.ready = false;
  }
  start() {
    if (!this.active || this.state.deleted) return;
    this.id = String(++serial); this.ready = false;
    source?.send({ action: 'subscribe', id: this.id, resource: this.resource });
  }
  disconnected(denied: boolean) {
    this.id = ''; this.ready = false; this.dirty = true; this.epoch++; this.controller?.abort(); clearTimeout(this.retry); this.retry = undefined;
    if (denied) this.publish({ data: undefined, head: undefined, error: t("acp.login_expired"), loading: false });
  }
  receive(head: Head, ready: boolean) {
    const old = this.state.head;
    if (old?.boot_id === head.boot_id && old.version > head.version && !head.deleted) {
      if (ready) { this.ready = true; void this.read(); }
      return;
    }
    if (old && (old.boot_id !== head.boot_id || old.session?.generation !== head.session?.generation)) {
      this.epoch++; this.controller?.abort(); this.dirty = true;
    }
    if (ready) this.ready = true;
    if (!ready && old?.boot_id === head.boot_id && old.version === head.version && !head.deleted) return;
    if (head.deleted) {
      this.epoch++; this.controller?.abort(); this.unsubscribe(); clearTimeout(this.retry); this.retry = undefined;
      this.publish({ head, deleted: true, data: undefined, error: '', loading: false });
      return;
    }
    if (!old || old.boot_id !== head.boot_id || old.version !== head.version || !this.state.data) this.dirty = true;
    this.publish({ head, deleted: false });
    void this.read();
  }
  revalidate() { this.forced = true; this.dirty = true; void this.read(); }
  refresh = () => {
    // Only an explicit refresh may bypass an outstanding failure's backoff.
    clearTimeout(this.retry); this.retry = undefined;
    this.revalidate();
  };
  accept(next: SessionSnapshot) {
    if (this.state.deleted || this.state.status === 'denied') return;
    const previous = this.state.data as SessionSnapshot | undefined;
    const head = this.state.head?.session;
    if (head && head.generation !== next.sync_generation) { this.revalidate(); return; }
    if (previous && previous.sync_generation === next.sync_generation && (previous.revision > next.revision
      || previous.revision === next.revision && (previous.overview || !next.overview))) {
      if (this.state.error || this.state.loading) this.publish({ error: '', loading: false });
      return;
    }
    this.publish({ data: next, loading: false, error: '' });
  }
  async current(): Promise<Data> {
    let changed = () => {};
    const release = this.subscribe(() => changed(), true);
    const boundary = () => new Promise<void>((resolve, reject) => {
      changed = () => {
        if (this.state.deleted || this.state.status === 'denied') reject(new Error(this.state.error || t("acp.resource_unavailable")));
        else if (this.ready) resolve();
      };
      changed();
    });
    try {
      await boundary();
      if (!this.pending) this.refresh();
      do {
        await this.pending;
        if (this.retry || this.state.error) throw new Error(this.state.error);
        if (!this.ready) await boundary();
      } while (this.pending || this.dirty);
      if (this.state.data === undefined) throw new Error(t("acp.resource_not_synced_yet"));
      return this.state.data;
    } finally { release(); }
  }
  private async load(signal: AbortSignal): Promise<Data | undefined> {
    const resource = this.resource;
    switch (resource.kind) {
      case 'session': {
        const current = this.state.data as SessionSnapshot | undefined;
        const head = this.state.head?.session;
        const base = head && current?.sync_generation !== head.generation ? undefined : current;
        return applySessionUpdates(base, await acpApi.updates(resource.thread_id, base, signal))
          ?? await acpApi.snapshot(resource.thread_id, signal);
      }
      case 'sessions': return acpApi.sessions(resource.cwd, signal);
      case 'connection': return acpApi.connection(resource.thread_id, signal);
      case 'requests': return acpApi.requests(resource.connection_id, signal);
      case 'operation': return resource.cwd === null ? acpApi.installationStatus(resource.agent_id, signal)
        : acpApi.connectionStatus(resource.agent_id, resource.cwd, signal);
    }
  }
  private read() {
    if (this.pending || this.retry || !this.active || !this.reading || !this.ready || this.state.deleted || !this.dirty) return this.pending;
    const head = this.state.head;
    const current = this.state.data as SessionSnapshot | undefined;
    if (!this.forced && this.resource.kind === 'session' && head?.session && current?.overview
      && current.sync_generation === head.session.generation && current.revision >= head.session.revision) {
      this.dirty = false; return;
    }
    const forced = this.forced;
    this.dirty = false; this.forced = false;
    const controller = new AbortController(); this.controller = controller;
    const epoch = this.epoch;
    this.publish({ loading: this.state.data === undefined });
    this.pending = this.load(controller.signal).then(data => {
      if (controller.signal.aborted || epoch !== this.epoch || this.state.deleted) return;
      if (this.resource.kind === 'session' && data) {
        const next = data as SessionSnapshot;
        const wanted = this.state.head?.session;
        if (wanted && wanted.generation !== next.sync_generation) {
          this.dirty = true; this.ready = false;
          if (this.active && this.id) source?.send({ action: 'subscribe', id: this.id, resource: this.resource });
          return;
        }
        this.accept(next);
        if (wanted && next.revision < wanted.revision) {
          this.dirty = true;
          if (current && current.sync_generation === next.sync_generation && current.revision >= next.revision) throw new Error(t("acp.waiting_for_the_server_to_commit_the_latest_session"));
        }
      } else {
        // A newer invalidation may have arrived during this read. Do not flash
        // a superseded snapshot; the single request loop will catch up below.
        if (head?.boot_id === this.state.head?.boot_id && head?.version === this.state.head?.version) this.publish({ data, loading: false, error: '' });
      }
      this.failures = 0;
    }).catch((error: unknown) => {
      if (controller.signal.aborted || epoch !== this.epoch) return;
      if (error instanceof AcpRequestError && (error.status === 401 || error.status === 403)) {
        this.disconnected(true); this.publish({ status: 'denied' }); return;
      }
      if (error instanceof AcpRequestError && (error.status === 404 || error.status === 410)) {
        this.unsubscribe(); this.publish({ deleted: true, loading: false, data: undefined }); return;
      }
      this.publish({ error: error instanceof Error ? error.message : String(error), loading: false });
      this.dirty = true;
      // A failed forced read still needs HTTP validation, even at the same revision.
      this.forced ||= forced;
      // Retry failed, outstanding work only; idle resources have no timers.
      this.retry = setTimeout(() => { this.retry = undefined; void this.read(); }, Math.min(30_000, 1000 * 2 ** Math.min(this.failures++, 5)));
    }).finally(() => {
      this.pending = undefined;
      if (!this.retry && this.dirty) queueMicrotask(() => { void this.read(); });
    });
    return this.pending;
  }
}

export function getResource(resource: Resource) {
  const key = resourceKey(resource);
  let store = stores.get(key);
  if (!store) { store = new ResourceStore(resource); stores.set(key, store); }
  return store;
}
export function acceptSession(snapshot: SessionSnapshot) {
  getResource({ kind: 'session', thread_id: snapshot.thread_id }).accept(snapshot);
}

export async function readSession(thread_id: string) {
  return await getResource({ kind: 'session', thread_id }).current() as SessionSnapshot;
}
