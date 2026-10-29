import { api } from '../../lib/api';
import type { AuthSession, Catalog, CheckResult, Connector, ConnectorFields } from './types';

const root = (org: string) => `/api/orgs/${encodeURIComponent(org)}`;
export const listConnectors = (org: string, signal?: AbortSignal) => api<Connector[]>(`${root(org)}/connectors`, { signal });
export const connectorCatalog = (org: string, signal?: AbortSignal) => api<Catalog>(`${root(org)}/connectors/catalog`, { signal });
export const getConnector = (org: string, id: string, signal?: AbortSignal) => api<Connector>(`${root(org)}/connectors/${id}`, { signal });
export const saveConnector = (org: string, fields: ConnectorFields, extra: { id?: string; revision?: number; api_key?: string; auth_session_id?: string; connect: boolean }) => api<Connector>(`${root(org)}/connectors${extra.id ? `/${extra.id}` : ''}`, { method: extra.id ? 'PUT' : 'POST', body: JSON.stringify({ ...fields, ...extra }) });
export const verifyConnector = (org: string, fields: ConnectorFields, extra: { connector_id?: string; api_key?: string; auth_session_id?: string }) => api<CheckResult>(`${root(org)}/connectors/verify`, { method: 'POST', body: JSON.stringify({ ...fields, ...extra }) });
export const checkConnector = (org: string, id: string) => api<Connector>(`${root(org)}/connectors/${id}/check`, { method: 'POST' });
export const enableConnector = (org: string, record: Connector) => api<Connector>(`${root(org)}/connectors/${record.id}/enabled`, { method: 'PUT', body: JSON.stringify({ enabled: !record.enabled, revision: record.revision }) });
export const removeConnector = (org: string, record: Connector) => api<void>(`${root(org)}/connectors/${record.id}?revision=${record.revision}`, { method: 'DELETE' });
export const startAuthentication = (org: string, fields: ConnectorFields) => api<AuthSession>(`${root(org)}/connector-auth`, { method: 'POST', body: JSON.stringify({ provider: fields.provider, executor: fields.executor }) });
export const authenticationStatus = (org: string, id: string) => api<AuthSession>(`${root(org)}/connector-auth/${id}`);
export const submitAuthentication = (org: string, id: string, code: string) => api<AuthSession>(`${root(org)}/connector-auth/${id}/code`, { method: 'POST', body: JSON.stringify({ code }) });
export const cancelAuthentication = (org: string, id: string) => api<{ status: string }>(`${root(org)}/connector-auth/${id}`, { method: 'DELETE' });
