import { useRef } from 'react';

/** Deduplicate retries of an unchanged payload; resource IDs come from the API. */
export function useRequestKey() {
  const pending = useRef<{ payload: string; key: string } | undefined>(undefined);
  return {
    keyFor(input: object) {
      const payload = JSON.stringify(input);
      if (pending.current?.payload !== payload) {
        // getRandomValues also works when AoW is opened over HTTP on a LAN.
        const key = Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join('');
        pending.current = { payload, key };
      }
      return pending.current.key;
    },
    reset() { pending.current = undefined; },
  };
}
