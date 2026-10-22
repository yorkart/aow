import { localizedFetch } from '../../lib/localizedFetch';
import { t } from '../../lib/i18n';
import { appUrl } from '../../lib/basePath';
import type { AuthStatus } from './types';

export async function authStatus(): Promise<AuthStatus> {
  const response = await localizedFetch(appUrl('/api/auth/status'), { cache: 'no-store' });
  if (!response.ok) throw new Error(t("auth.unable_to_read_login_status_http", { value0: response.status }));
  return response.json() as Promise<AuthStatus>;
}

export async function login(method: string, credentials: Record<string, unknown>): Promise<AuthStatus> {
  const response = await localizedFetch(appUrl('/api/auth/login'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...credentials, method }),
  });
  const payload = await response.json().catch(() => null) as AuthStatus | null;
  if (!response.ok) throw new Error(payload?.message ?? t("auth.sign_in_failed_http", { value0: response.status }));
  if (!payload?.authenticated) throw new Error(t("auth.sign_in_incomplete_try_again"));
  return payload;
}

export async function logout(): Promise<void> {
  const response = await localizedFetch(appUrl('/api/auth/logout'), { method: 'POST', cache: 'no-store' });
  if (!response.ok) throw new Error(t("auth.sign_out_failed_http", { value0: response.status }));
}
