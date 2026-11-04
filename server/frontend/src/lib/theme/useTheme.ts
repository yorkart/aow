import { useSyncExternalStore } from 'react';
import { getTheme, subscribeTheme } from './store';

export function useTheme() {
  return useSyncExternalStore(subscribeTheme, getTheme, getTheme);
}
