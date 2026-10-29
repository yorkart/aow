const apiOrigin = (import.meta.env.VITE_ORBIT_API_URL ?? '').replace(/\/+$/, '');

export function apiUrl(path: string): string { return `${apiOrigin}${path}`; }

export class ApiError extends Error {
  constructor(message: string, readonly code?: string) { super(message); }
}

export async function api<T>(path: string, options: RequestInit = {}): Promise<T> {
  const response = await fetch(apiUrl(path), {
    ...options,
    credentials: 'include',
    cache: 'no-store',
    headers: { ...(options.body ? { 'Content-Type': 'application/json' } : {}), ...options.headers },
  });
  if (response.status === 401) window.dispatchEvent(new Event('orbit:unauthorized'));
  if (!response.ok) {
    const error = await response.json().catch(() => null) as { message?: string; code?: string } | null;
    throw new ApiError(error?.message ?? `请求失败（HTTP ${response.status}）`, error?.code);
  }
  if (response.status === 204) return undefined as T;
  return response.json() as Promise<T>;
}
