import { appUrl } from './basePath';

export class HttpRequestError extends Error {}

export async function fetchWithTimeout(url: string, init?: RequestInit, timeoutMs = 12_000) {
  const controller = new AbortController();
  const signal = init?.signal ? AbortSignal.any([init.signal, controller.signal]) : controller.signal;
  const timer = window.setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(appUrl(url), { ...init, signal });
  } catch (error) {
    init?.signal?.throwIfAborted();
    if (error instanceof DOMException && error.name === 'AbortError') {
      throw new Error(`请求超时（${Math.round(timeoutMs / 1000)} 秒）`);
    }
    throw error;
  } finally {
    window.clearTimeout(timer);
  }
}

export async function request<T>(url: string, init?: RequestInit, retries = 1, timeoutMs = 12_000): Promise<T> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      init?.signal?.throwIfAborted();
      const response = await fetchWithTimeout(url, init, timeoutMs);
      const payload = await response.json().catch(() => null);
      init?.signal?.throwIfAborted();
      if (!response.ok) {
        throw new HttpRequestError(payload?.message ?? payload?.error?.message ?? `HTTP ${response.status}`);
      }
      return payload as T;
    } catch (error) {
      init?.signal?.throwIfAborted();
      if (error instanceof HttpRequestError || attempt >= retries) throw error;
      await new Promise((resolve) => window.setTimeout(resolve, 250));
    }
  }
}
