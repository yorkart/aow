import { useCallback, useSyncExternalStore } from 'react';
import { terminalStore } from './terminalStore';

export function useTerminalState(enabled: boolean) {
  const subscribe = useCallback((listener: () => void) => enabled ? terminalStore.subscribe(listener) : () => {}, [enabled]);
  return useSyncExternalStore(subscribe, terminalStore.snapshot);
}
