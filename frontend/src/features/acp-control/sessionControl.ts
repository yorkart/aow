import { getLanguage } from '../../lib/i18n';
// AoW page ownership, independent of the ACP protocol and the Upstream port.
import { useMemo, useSyncExternalStore } from 'react';
import { appUrl } from '../../lib/basePath';

export type ControlState = { state: 'connecting' | 'claimed' | 'observing' | 'disconnected'; token?: string };
const initial: ControlState = { state: 'connecting' };
const controls = new Map<string, ReturnType<typeof createControl>>();

function createControl(threadId: string, key: string) {
  let value = initial;
  let socket: WebSocket | undefined;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let dispose: ReturnType<typeof setTimeout> | undefined;
  let observer = false;
  const listeners = new Set<() => void>();
  const publish = (next: ControlState) => {
    if (value.state === next.state && value.token === next.token) return;
    value = next;
    for (const listener of listeners) listener();
  };
  const connect = () => {
    if (socket || !listeners.size) return;
    const url = new URL(appUrl(`/api/acp/sessions/${encodeURIComponent(threadId)}/control`), location.href);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    url.searchParams.set('lang', getLanguage());
    const current = new WebSocket(url);
    socket = current;
    publish(initial);
    current.onopen = () => {
      if (socket === current) current.send(JSON.stringify(observer ? { type: 'observe' } : { type: 'claim', force: false }));
    };
    current.onmessage = event => {
      if (socket !== current) return;
      try {
        const next: unknown = JSON.parse(String(event.data));
        if (!next || typeof next !== 'object' || !('state' in next)) return;
        if (next.state === 'claimed' && 'token' in next && typeof next.token === 'string' && next.token) {
          observer = false; publish({ state: 'claimed', token: next.token });
        } else if (next.state === 'observing') {
          observer = true; publish({ state: 'observing' });
        }
      } catch { current.close(); }
    };
    current.onclose = () => {
      if (socket !== current) return;
      socket = undefined; publish({ state: 'disconnected' });
      if (listeners.size) retry = setTimeout(connect, 1500);
    };
    current.onerror = () => current.close();
  };
  return {
    threadId,
    read: () => value,
    subscribe: (listener: () => void) => {
      clearTimeout(dispose); listeners.add(listener); connect();
      return () => {
        listeners.delete(listener);
        // Portal moves and StrictMode remounts retain the same attachment.
        if (!listeners.size) dispose = setTimeout(() => {
          clearTimeout(retry);
          const previous = socket; socket = undefined; previous?.close();
          controls.delete(key);
        }, 0);
      };
    },
    takeover: () => {
      if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: 'claim', force: true }));
    },
  };
}

const empty = { read: () => initial, subscribe: () => () => {}, takeover: () => {} };

export function useAcpControl(threadId: string | undefined, viewId: string) {
  const control = useMemo(() => {
    if (!threadId) return empty;
    const key = JSON.stringify([threadId, viewId]);
    let control = controls.get(key);
    if (!control) { control = createControl(threadId, key); controls.set(key, control); }
    return control;
  }, [threadId, viewId]);
  return { ...useSyncExternalStore(control.subscribe, control.read), takeover: control.takeover };
}

// Sidebar deletion can use this page's ownership; a different page's credential
// is never persisted or broadcast through localStorage.
export function acpControlHeaders(threadId: string): Record<string, string> {
  for (const control of controls.values()) {
    const token = control.threadId === threadId ? control.read().token : undefined;
    if (token) return { 'X-Aow-Acp-Control': token };
  }
  return {};
}
