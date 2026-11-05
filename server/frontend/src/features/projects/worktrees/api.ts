import type { WorktreeCreationJob, WorktreeColor, WorktreeIconId, AowProject, ProjectBranches, WorktreeRemovalPreview, WorktreeRemovalJob } from '../../../aow/types';
import { aowRequest } from '../../../lib/aowRequest';

export const worktreesApi = {
  projectBranches: (id: string, signal?: AbortSignal) => aowRequest<ProjectBranches>(`/api/projects/${encodeURIComponent(id)}/branches`, { signal, cache: 'no-store' }),
  worktreeCreations: () => aowRequest<WorktreeCreationJob[]>('/api/worktree-creations', { cache: 'no-store' }),
  createWorktree: (id: string, input: { branch: string; baseRef: string; path: string; pullFirst: boolean }) => aowRequest<WorktreeCreationJob>(`/api/projects/${encodeURIComponent(id)}/worktrees`, {
    method: 'POST',
    body: JSON.stringify({ branch: input.branch, base_ref: input.baseRef, path: input.path, pull_first: input.pullFirst }),
  }),
  setWorktreeColor: (id: string, path: string, color: WorktreeColor) => aowRequest<AowProject>(`/api/projects/${encodeURIComponent(id)}/worktrees/color`, {
    method: 'POST',
    body: JSON.stringify({ path, color }),
  }),
  setWorktreeIcon: (id: string, path: string, icon: WorktreeIconId) => aowRequest<AowProject>(`/api/projects/${encodeURIComponent(id)}/worktrees/icon`, {
    method: 'POST',
    body: JSON.stringify({ path, icon }),
  }),
  inspectWorktreeRemoval: (id: string, path: string, signal?: AbortSignal) => {
    const query = new URLSearchParams({ path });
    return aowRequest<WorktreeRemovalPreview>(`/api/projects/${encodeURIComponent(id)}/worktrees/removal?${query}`, { signal });
  },
  removeWorktree: (id: string, path: string, force: boolean) => {
    const query = new URLSearchParams({ path, force: String(force) });
    return aowRequest<WorktreeRemovalJob>(`/api/projects/${encodeURIComponent(id)}/worktrees/removal?${query}`, { method: 'DELETE' });
  },
  removeWorktrees: (id: string, items: { path: string; force: boolean }[]) => aowRequest<WorktreeRemovalJob>(`/api/projects/${encodeURIComponent(id)}/worktrees/removals`, {
    method: 'POST', body: JSON.stringify({ items }),
  }),
  worktreeRemovals: () => aowRequest<WorktreeRemovalJob[]>('/api/worktree-removals', { cache: 'no-store' }),
};
