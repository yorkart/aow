import { api } from './lib/api';

export interface Org { id: string; name: string; created_at: string }
export interface OrgState { orgs: Org[]; active_org_id: string | null }

export const loadOrgs = (signal?: AbortSignal) => api<OrgState>('/api/orgs', { signal });
export const createOrg = (name: string) => api<OrgState>('/api/orgs', { method: 'POST', body: JSON.stringify({ name }) });
export const selectOrg = (id: string) => api<OrgState>('/api/orgs/active', { method: 'PUT', body: JSON.stringify({ id }) });
