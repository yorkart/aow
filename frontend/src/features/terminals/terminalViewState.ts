import { useEffect, useSyncExternalStore } from 'react';
import type { TerminalTab } from './types';
import { terminalTabState, type TerminalConnectionState } from './terminalState';

// Browser-local view state is independent of the terminal process lifecycle.
// Sharing it here also covers tabs displayed in another worktree or a portal.
const listeners = new Set<() => void>();
let connections: ReadonlyMap<string, TerminalConnectionState> = new Map();
const paneKey = (tabId: string, paneId: string) => JSON.stringify([tabId, paneId]);
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
};
const snapshot = () => connections;

const selectionListeners = new Set<() => void>();
let requestedPane: { tabId: string; paneId: string } | undefined;
const selectionSnapshot = () => requestedPane;
const subscribeSelection = (listener: () => void) => {
  selectionListeners.add(listener);
  return () => { selectionListeners.delete(listener); };
};

export function requestTerminalPaneSelection(tabId: string, paneId: string) {
  requestedPane = { tabId, paneId };
  for (const listener of selectionListeners) listener();
}

export function useTerminalPaneSelection() {
  return useSyncExternalStore(subscribeSelection, selectionSnapshot);
}

function update(key: string, state?: TerminalConnectionState) {
  if (connections.get(key) === state) return;
  const next = new Map(connections);
  if (state === undefined) next.delete(key);
  else next.set(key, state);
  connections = next;
  for (const listener of listeners) listener();
}

export function usePublishTerminalConnection(tabId: string, paneId: string, state: TerminalConnectionState) {
  const key = paneKey(tabId, paneId);
  useEffect(() => { update(key, state); }, [key, state]);
  useEffect(() => () => update(key), [key]);
}

export function useTerminalStates() {
  const current = useSyncExternalStore(subscribe, snapshot);
  return (tab: TerminalTab, opened: boolean) => terminalTabState(tab, opened, paneId => current.get(paneKey(tab.id, paneId)));
}
