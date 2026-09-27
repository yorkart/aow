import type { RepositorySummary, GitIgnoredPaths, GitStatus, GitLog, GitCommitFiles, GitCommitDetail, GitDiff } from './types';
import { request } from '../../lib/http';

export const gitApi = {
  repositories: (root: string, signal?: AbortSignal) => request<RepositorySummary[]>(`/api/git/repositories?root=${encodeURIComponent(root)}&depth=5`, { signal }),
  gitIgnored: (root: string, paths: string[]) => request<GitIgnoredPaths>('/api/git/ignored', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ root, paths }),
  }, 0),
  gitStatus: (repo: string, signal?: AbortSignal) => request<GitStatus>(`/api/git/status?repo=${encodeURIComponent(repo)}`, { signal, cache: 'no-store' }, signal ? 0 : 1),
  gitSync: (repo: string, command: 'pull' | 'push') => request<void>(`/api/git/${command}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ repo }),
  }, 0, 130_000),
  gitLog: (repo: string, signal?: AbortSignal) => request<GitLog>(`/api/git/log?repo=${encodeURIComponent(repo)}&limit=100`, { signal, cache: 'no-store' }, signal ? 0 : 1),
  gitDiff: (repo: string, path?: string, staged = false, signal?: AbortSignal) => {
    const query = new URLSearchParams({ repo, staged: String(staged) });
    if (path) query.set('path', path);
    return request<GitDiff>(`/api/git/diff?${query}`, { signal, cache: 'no-store' });
  },
  gitCommitFiles: (repo: string, commit: string) => request<GitCommitFiles>(`/api/git/commit/files?repo=${encodeURIComponent(repo)}&commit=${encodeURIComponent(commit)}`),
  gitCommitDetail: (repo: string, commit: string) => request<GitCommitDetail>(`/api/git/commit/detail?repo=${encodeURIComponent(repo)}&commit=${encodeURIComponent(commit)}`),
  gitCommitDiff: (repo: string, commit: string, path: string, originalPath?: string, signal?: AbortSignal) => {
    const query = new URLSearchParams({ repo, commit, path });
    if (originalPath) query.set('original_path', originalPath);
    return request<GitDiff>(`/api/git/commit/diff?${query}`, { signal, cache: 'no-store' });
  },
};
