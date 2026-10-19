import { useEffect, useRef } from 'react';
import type { OpenDocument } from './types';

// The caller supplies only the focused workspace's selected diff, including
// floating tabs. Hidden documents never own a polling timer.
export function useActiveDiffRefresh(target: OpenDocument | undefined, refresh: (id: string, signal: AbortSignal) => Promise<void>) {
  const latestRefresh = useRef(refresh);
  latestRefresh.current = refresh;
  const id = target?.id;
  const instanceId = target?.instanceId;

  useEffect(() => {
    if (!id) return;
    let disposed = false;
    let timer: number | undefined;
    let controller: AbortController | undefined;
    const run = async () => {
      if (disposed || controller || document.visibilityState !== 'visible') return;
      const request = new AbortController();
      controller = request;
      try {
        await latestRefresh.current(id, request.signal);
      } finally {
        if (controller === request) controller = undefined;
      }
    };
    const stop = () => {
      window.clearInterval(timer);
      timer = undefined;
      controller?.abort();
      controller = undefined;
    };
    const onVisibilityChange = () => {
      stop();
      if (document.visibilityState !== 'visible') return;
      void run();
      timer = window.setInterval(() => void run(), 3_000);
    };
    document.addEventListener('visibilitychange', onVisibilityChange);
    onVisibilityChange();
    return () => {
      disposed = true;
      stop();
      document.removeEventListener('visibilitychange', onVisibilityChange);
    };
  }, [id, instanceId]);
}
