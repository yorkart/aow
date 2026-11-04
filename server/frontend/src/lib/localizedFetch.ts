import { languageHeaders } from './i18n';

/** Keep locale propagation local to application requests; do not patch global fetch. */
export function localizedFetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> {
  const headers = languageHeaders(init?.headers ?? (input instanceof Request ? input.headers : undefined));
  return fetch(input, { ...init, headers });
}
