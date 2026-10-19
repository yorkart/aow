import { useCallback, useMemo, useSyncExternalStore } from 'react';
import type { ConnectionInfo, ConnectionStatus, Permission, SessionInfo, SessionSnapshot } from '../types';
import { getResource, type Resource, type SyncState } from './store';

const empty: SyncState<never> = { error: '', deleted: false, loading: false, status: 'connecting' };
const emptySnapshot = () => empty;
function useResource<T>(resource: Resource | undefined, read = true) {
  const key = resource && JSON.stringify(resource);
  // Resource identity is its canonical key, independent of the caller's render.
  const store = useMemo(() => key ? getResource(JSON.parse(key) as Resource) : undefined, [key]);
  const subscribe = useCallback((listener: () => void) => store?.subscribe(listener, read) ?? (() => undefined), [store, read]);
  const state = useSyncExternalStore(subscribe, store?.snapshot ?? emptySnapshot) as SyncState<T>;
  return { ...state, reload: store?.refresh ?? (() => undefined) };
}
export const useSessionSync = (id: string | undefined, visible: boolean) => useResource<SessionSnapshot>(id ? { kind: 'session', thread_id: id } : undefined, visible);
export const useConnectionSync = (id: string | undefined) => useResource<ConnectionInfo | null>(id ? { kind: 'connection', thread_id: id } : undefined);
export const useRequestsSync = (id: string | undefined) => useResource<Permission[]>(id ? { kind: 'requests', connection_id: id } : undefined);
export const useSessionsSync = (cwd: string, active = true) => useResource<SessionInfo[]>(active ? { kind: 'sessions', cwd } : undefined);
export const useOperationSync = (agentId: string, cwd?: string) => useResource<ConnectionStatus | null>(agentId ? { kind: 'operation', agent_id: agentId, cwd: cwd ?? null } : undefined);
