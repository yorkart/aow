import type { AowProject } from '../../aow/types';
import { aowRequest } from '../../lib/aowRequest';

export const projectsApi = {
  projects: () => aowRequest<AowProject[]>('/api/projects'),
  projectAvatar: (id: string, signal?: AbortSignal) => aowRequest<{ avatar_url: string | null }>(`/api/projects/${encodeURIComponent(id)}/avatar`, { signal, cache: 'no-store' }),
  registerProject: (path: string, name?: string, notesPath?: string) => aowRequest<AowProject>('/api/projects', {
    method: 'POST',
    body: JSON.stringify({ path, ...(name?.trim() ? { name: name.trim() } : {}), ...(notesPath?.trim() ? { notes_path: notesPath.trim() } : {}) }),
  }),
  refreshProject: (id: string) => aowRequest<AowProject>(`/api/projects/${encodeURIComponent(id)}/refresh`, { method: 'POST' }),
  bindNotes: (id: string, path: string) => aowRequest<AowProject>(`/api/projects/${encodeURIComponent(id)}/notes/bind`, {
    method: 'POST',
    body: JSON.stringify({ path }),
  }),
  createTemporaryNote: (id: string, extension: 'md' | 'txt') => aowRequest<{ path: string; name: string; kind: 'file' }>(`/api/projects/${encodeURIComponent(id)}/notes/temporary`, {
    method: 'POST',
    body: JSON.stringify({ extension }),
  }),
  removeProject: (id: string) => aowRequest<void>(`/api/projects/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};
