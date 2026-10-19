import { useEffect, useState } from 'react';
import { HttpRequestError } from '../../lib/http';
import { filesApi } from './api';
import type { SearchOptions, SearchState } from './searchTypes';

export function useFileSearch(root: string, query: string, options: SearchOptions, enabled: boolean, refresh: number): SearchState {
  const key = JSON.stringify([root, query, options, enabled, refresh]);
  const [state, setState] = useState<SearchState>({ key: '', loading: false });
  const active = enabled && Boolean(query.trim());
  useEffect(() => {
    if (!active) return;
    const controller = new AbortController();
    const bytes = crypto.getRandomValues(new Uint8Array(16));
    bytes[6] = (bytes[6] & 15) | 64; bytes[8] = (bytes[8] & 63) | 128;
    const hex = Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
    const requestId = `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
    let sent = false;
    let finished = false;
    setState({ key, loading: true });
    const timer = window.setTimeout(() => {
      sent = true;
      void filesApi.search({ root, query, ...options, request_id: requestId }, controller.signal)
        .then(result => { if (!controller.signal.aborted) setState({ key, loading: false, result }); })
        .catch((error: Error) => {
          if (controller.signal.aborted) return;
          // A timeout or transport failure can leave the server waiting for a search slot.
          if (!(error instanceof HttpRequestError)) void filesApi.cancelSearch(requestId).catch(() => undefined);
          setState({ key, loading: false, error: error.message });
        })
        .finally(() => { finished = true; });
    }, 300);
    return () => {
      window.clearTimeout(timer);
      controller.abort();
      if (sent && !finished) void filesApi.cancelSearch(requestId).catch(() => undefined);
    };
  }, [active, key, root, query, options]);
  return active ? state.key === key ? state : { key, loading: true } : { key, loading: false };
}
