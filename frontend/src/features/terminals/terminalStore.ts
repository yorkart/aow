import { LiveEvents } from '../../lib/liveEvents';
import { agentsFromPayload, tabFromPayload, terminalApi } from './terminalApi';
import type { TerminalAgentList, TerminalPaneStatus, TerminalTab } from './types';

export const emptyTerminalMetadata: TerminalAgentList = { agents: {}, titles: {}, processes: {}, activity: {} };
export interface TerminalState {
  tabs?: TerminalTab[];
  metadata: TerminalAgentList;
  error: string;
  status: 'connecting' | 'live' | 'offline' | 'degraded' | 'denied';
}

let state: TerminalState = { metadata: emptyTerminalMetadata, error: '', status: 'connecting' };
const listeners = new Set<() => void>();
let source: LiveEvents | undefined;
let protocol = false;
let subscribed = false;
let bootId: string | undefined;
let revision = -1;
let serial = 0;
let barrier: string | undefined;
let epoch = 0;
let session = 0;
let pending: Promise<void> | undefined;
let controller: AbortController | undefined;
let fallback: ReturnType<typeof setInterval> | undefined;
let greeting: ReturnType<typeof setTimeout> | undefined;

function publish(change: Partial<TerminalState>) {
  if (Object.entries(change).every(([key, value]) => state[key as keyof TerminalState] === value)) return;
  state = { ...state, ...change };
  for (const listener of listeners) listener();
}

function stableTabs(tabs: TerminalTab[]) {
  const previous = new Map(state.tabs?.map(tab => [tab.id, tab]));
  const next = tabs.map(tab => {
    const old = previous.get(tab.id);
    return old && JSON.stringify(old) === JSON.stringify(tab) ? old : tab;
  });
  return state.tabs?.length === next.length && next.every((tab, index) => tab === state.tabs?.[index]) ? state.tabs : next;
}

function invalidateRead() { epoch++; controller?.abort(); }
function stopFallback() { clearInterval(fallback); fallback = undefined; clearTimeout(greeting); }
function denied(error: string) {
  session++;
  invalidateRead(); stopFallback(); barrier = undefined; subscribed = false;
  publish({ tabs: undefined, metadata: emptyTerminalMetadata, error, status: 'denied' });
}

function readFallback() {
  if (pending) return pending;
  if (!listeners.size || document.visibilityState !== 'visible' || state.status === 'denied') return Promise.resolve();
  const request = new AbortController(); controller = request;
  const generation = epoch;
  pending = Promise.allSettled([terminalApi.list(undefined, request.signal), terminalApi.agents(undefined, request.signal)]).then(([tabs, metadata]) => {
    if (generation !== epoch || request.signal.aborted) return;
    for (const result of [tabs, metadata]) {
      if (result.status === 'rejected' && [401, 403].includes(result.reason?.status)) { denied('登录已失效'); return; }
    }
    const change: Partial<TerminalState> = {};
    if (tabs.status === 'fulfilled') { change.tabs = stableTabs(tabs.value); change.error = ''; }
    else change.error = tabs.reason instanceof Error ? tabs.reason.message : String(tabs.reason);
    if (metadata.status === 'fulfilled' && JSON.stringify(metadata.value) !== JSON.stringify(state.metadata)) change.metadata = metadata.value;
    publish(change);
  }).finally(() => { pending = undefined; });
  return pending;
}

function startFallback(status: 'offline' | 'degraded') {
  if (!source || state.status === 'denied') return;
  publish({ status });
  void readFallback();
  if (!fallback) fallback = setInterval(() => { void readFallback(); }, 30_000);
}

function subscribeLive() {
  if (protocol && !subscribed && document.visibilityState === 'visible') {
    subscribed = source?.send({ action: 'subscribe_terminals' }) ?? false;
  }
}

function visibilityChanged() {
  if (document.visibilityState !== 'visible') {
    if (subscribed) source?.send({ action: 'unsubscribe_terminals' });
    subscribed = false; barrier = undefined; invalidateRead();
  } else if (protocol) subscribeLive();
  else if (state.status === 'offline' || state.status === 'degraded') void readFallback();
}

function connect() {
  if (source) return;
  const connection = new LiveEvents('terminals');
  source = connection;
  document.addEventListener('visibilitychange', visibilityChanged);
  connection.onopen = () => {
    protocol = false; subscribed = false; barrier = undefined; stopFallback(); invalidateRead();
    publish({ status: 'connecting' });
    greeting = setTimeout(() => startFallback('degraded'), 2000);
  };
  connection.onerror = () => {
    protocol = false; subscribed = false; barrier = undefined; stopFallback(); invalidateRead();
    if (connection.readyState === 2) denied('登录已失效');
    else startFallback('offline');
  };
  connection.addEventListener('terminals', event => {
    if (source !== connection) return;
    let message;
    try { message = JSON.parse((event as MessageEvent<string>).data); } catch { return; }
    if (message?.type === 'hello' && message.protocol === 1) {
      protocol = true; stopFallback(); subscribeLive();
      return;
    }
    if (!protocol || !subscribed || message?.type !== 'snapshot') return;
    const next = message.state;
    if (!next || typeof next.boot_id !== 'string' || !Number.isSafeInteger(next.revision) || next.revision < 0) return;
    if (barrier && message.request_id !== barrier) return;
    if (bootId === next.boot_id && revision > next.revision) return;
    let metadata: TerminalAgentList | undefined;
    let tabs: TerminalTab[] | undefined;
    try {
      if (next.metadata != null) metadata = agentsFromPayload(next.metadata);
      if (next.tabs != null) {
        if (!Array.isArray(next.tabs)) return;
        const normalized = next.tabs.map(tabFromPayload);
        if (normalized.some((tab: TerminalTab | null) => !tab)) return;
        tabs = normalized;
      }
    } catch { return; }
    barrier = undefined; bootId = next.boot_id; revision = next.revision;
    invalidateRead(); stopFallback();
    publish({
      status: 'live', error: typeof next.error === 'string' ? next.error : '',
      ...(tabs && { tabs: stableTabs(tabs) }),
      ...(metadata && JSON.stringify(metadata) !== JSON.stringify(state.metadata) ? { metadata } : {}),
    });
  });
}

export const terminalStore = {
  snapshot: () => state,
  session: () => session,
  isCurrentSession: (generation: number) => generation === session && state.status !== 'denied',
  subscribe(listener: () => void) {
    listeners.add(listener); connect();
    return () => {
      listeners.delete(listener);
      // Keep a subscription across StrictMode remounts and portal moves.
      queueMicrotask(() => {
        if (listeners.size) return;
        if (subscribed) source?.send({ action: 'unsubscribe_terminals' });
        source?.close(); source = undefined; subscribed = false; protocol = false; barrier = undefined;
        session++; invalidateRead(); stopFallback();
        document.removeEventListener('visibilitychange', visibilityChanged);
        state = { metadata: emptyTerminalMetadata, error: '', status: 'connecting' };
        bootId = undefined; revision = -1;
      });
    };
  },
  refresh(): Promise<void> {
    if (protocol && subscribed) {
      const id = String(++serial);
      if (source?.send({ action: 'refresh_terminals', id })) { barrier = id; return Promise.resolve(); }
    }
    return readFallback();
  },
  update(transform: (tabs: TerminalTab[]) => TerminalTab[]) {
    if (state.status === 'denied') return;
    invalidateRead();
    if (state.tabs) publish({ tabs: stableTabs(transform(state.tabs)), error: '' });
    // Ignore snapshots queued before a successful HTTP mutation until the server
    // acknowledges the new boundary on the ordered WebSocket connection.
    void this.refresh();
  },
  replace(tab: TerminalTab) {
    this.update(tabs => tabs.some(item => item.id === tab.id)
      ? tabs.map(item => item.id === tab.id ? tab : item) : [...tabs, tab]);
  },
  remove(id: string) { this.update(tabs => tabs.filter(tab => tab.id !== id)); },
  updatePaneStatus(tabId: string, paneId: string, status: TerminalPaneStatus, exitCode?: number | null) {
    if (state.tabs) publish({ tabs: state.tabs.map(tab => tab.id === tabId ? { ...tab, panes: tab.panes.map(pane => pane.id === paneId
      ? { ...pane, status, ...(exitCode === undefined ? {} : { exit_code: exitCode }) } : pane) } : tab) });
  },
};
