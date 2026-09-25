import type { AowWorktree, WorktreeColor, WorktreeIconId, AowProject, AowSettings, WorktreeRemovalPreview, WorktreeRemovalJob, PinnedWorktrees, PinnedWorktreesUpdate } from './types';
import { aowRequest } from '../lib/aowRequest';

export const aowApi = {
  pinnedWorktrees: () => aowRequest<PinnedWorktrees>('/api/aow/pinned-worktrees', { cache: 'no-store' }),
  updatePinnedWorktrees: (input: PinnedWorktreesUpdate) => aowRequest<PinnedWorktrees>('/api/aow/pinned-worktrees', {
    method: 'PATCH', body: JSON.stringify(input),
  }),
  settings: () => aowRequest<AowSettings>('/api/aow/settings', { cache: 'no-store' }),
  discoveredPath: () => aowRequest<string[]>('/api/aow/settings/discovered-path', { cache: 'no-store' }),
  updateSettings: (input: { notesBase?: string; nodeAddresses?: string[]; executionPath?: string[]; editor?: AowSettings['editor'] }) => aowRequest<AowSettings>('/api/aow/settings', {
    method: 'PUT',
    body: JSON.stringify({ notes_base: input.notesBase?.trim(), node_addresses: input.nodeAddresses, execution_path: input.executionPath, editor: input.editor }),
  }),
  projects: () => aowRequest<AowProject[]>('/api/aow/projects'),
  projectAvatar: (id: string, signal?: AbortSignal) => aowRequest<{ avatar_url: string | null }>(`/api/aow/projects/${encodeURIComponent(id)}/avatar`, { signal, cache: 'no-store' }),
  registerProject: (path: string, name?: string, notesPath?: string) => aowRequest<AowProject>('/api/aow/projects', {
    method: 'POST',
    body: JSON.stringify({ path, ...(name?.trim() ? { name: name.trim() } : {}), ...(notesPath?.trim() ? { notes_path: notesPath.trim() } : {}) }),
  }),
  refreshProject: (id: string) => aowRequest<AowProject>(`/api/aow/projects/${encodeURIComponent(id)}/refresh`, { method: 'POST' }),
  bindNotes: (id: string, path: string) => aowRequest<AowProject>(`/api/aow/projects/${encodeURIComponent(id)}/notes/bind`, {
    method: 'POST',
    body: JSON.stringify({ path }),
  }),
  createTemporaryNote: (id: string, extension: 'md' | 'txt') => aowRequest<{ path: string; name: string; kind: 'file' }>(`/api/aow/projects/${encodeURIComponent(id)}/notes/temporary`, {
    method: 'POST',
    body: JSON.stringify({ extension }),
  }),
  createWorktree: (id: string, input: { branch: string; baseRef: string; path: string; pullFirst: boolean }) => aowRequest<{ project: AowProject; worktree: AowWorktree }>(`/api/aow/projects/${encodeURIComponent(id)}/worktrees`, {
    method: 'POST',
    body: JSON.stringify({ branch: input.branch, base_ref: input.baseRef, path: input.path, pull_first: input.pullFirst }),
  }),
  setWorktreeColor: (id: string, path: string, color: WorktreeColor) => aowRequest<AowProject>(`/api/aow/projects/${encodeURIComponent(id)}/worktrees/color`, {
    method: 'POST',
    body: JSON.stringify({ path, color }),
  }),
  setWorktreeIcon: (id: string, path: string, icon: WorktreeIconId) => aowRequest<AowProject>(`/api/aow/projects/${encodeURIComponent(id)}/worktrees/icon`, {
    method: 'POST',
    body: JSON.stringify({ path, icon }),
  }),
  inspectWorktreeRemoval: (id: string, path: string, signal?: AbortSignal) => {
    const query = new URLSearchParams({ path });
    return aowRequest<WorktreeRemovalPreview>(`/api/aow/projects/${encodeURIComponent(id)}/worktrees/removal?${query}`, { signal });
  },
  removeWorktree: (id: string, path: string, force: boolean) => {
    const query = new URLSearchParams({ path, force: String(force) });
    return aowRequest<WorktreeRemovalJob>(`/api/aow/projects/${encodeURIComponent(id)}/worktrees/removal?${query}`, { method: 'DELETE' });
  },
  removeWorktrees: (id: string, items: { path: string; force: boolean }[]) => aowRequest<WorktreeRemovalJob>(`/api/aow/projects/${encodeURIComponent(id)}/worktrees/removals`, {
    method: 'POST', body: JSON.stringify({ items }),
  }),
  worktreeRemovals: () => aowRequest<WorktreeRemovalJob[]>('/api/aow/worktree-removals', { cache: 'no-store' }),
  removeProject: (id: string) => aowRequest<void>(`/api/aow/projects/${encodeURIComponent(id)}`, { method: 'DELETE' }),
};
