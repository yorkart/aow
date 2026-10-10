import { appUrl } from '../../lib/basePath';
import { object, text } from './types';

export class AcpRequestError extends Error {
  constructor(message: string, readonly code: string) { super(message); }
}
export const authenticationRequired = (reason: unknown) => reason instanceof AcpRequestError && reason.code === 'acp_auth_required';

export async function acpRequest<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(appUrl(url), { ...init, headers: init?.body === undefined ? init?.headers : { 'Content-Type': 'application/json', ...init.headers } });
  const payload: unknown = response.status === 204 ? null : await response.json().catch(() => null);
  if (!response.ok) throw new AcpRequestError(text(object(payload).message) || `HTTP ${response.status}`, text(object(payload).code));
  return payload as T;
}
