import { appUrl } from '../../lib/basePath';
import type { AuthStatus } from './types';

export async function authStatus(): Promise<AuthStatus> {
  const response = await fetch(appUrl('/api/auth/status'), { cache: 'no-store' });
  if (!response.ok) throw new Error(`无法读取登录状态（HTTP ${response.status}）`);
  return response.json() as Promise<AuthStatus>;
}

export async function login(method: string, credentials: Record<string, unknown>): Promise<AuthStatus> {
  const response = await fetch(appUrl('/api/auth/login'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...credentials, method }),
  });
  const payload = await response.json().catch(() => null) as AuthStatus | null;
  if (!response.ok) throw new Error(payload?.message ?? `登录失败（HTTP ${response.status}）`);
  if (!payload?.authenticated) throw new Error('登录未完成，请重试。');
  return payload;
}
