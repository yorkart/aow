import { appUrl } from './basePath';

class AowHttpError extends Error {}

export async function aowRequest<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(appUrl(url), {
    ...init,
    headers: init?.body === undefined
      ? init?.headers
      : { 'Content-Type': 'application/json', ...init.headers },
  });
  const payload = response.status === 204 ? null : await response.json().catch(() => null);
  if (!response.ok) {
    const message = payload && typeof payload === 'object' && 'message' in payload
      ? String(payload.message)
      : `HTTP ${response.status}`;
    throw new AowHttpError(message);
  }
  return payload as T;
}
