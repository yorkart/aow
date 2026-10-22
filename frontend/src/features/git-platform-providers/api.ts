import type { GitPlatformProvider, GitPlatformProviderSettings, GitPlatformTarget } from './types';
import { aowRequest } from '../../lib/aowRequest';
import { request } from '../../lib/http';

export const gitPlatformApi = {
  targets: (repo: string, operations: string[] = []) => request<GitPlatformTarget[]>('/api/git-platform-targets?' + new URLSearchParams({ repo, ...(operations.length ? { operations: operations.join(',') } : {}) }), undefined, 0, 65_000),
  providers: () => aowRequest<GitPlatformProviderSettings>('/api/aow/git-platform-providers'),
  saveProviders: (settings: GitPlatformProviderSettings) => aowRequest<GitPlatformProviderSettings>('/api/aow/git-platform-providers', { method: 'PUT', body: JSON.stringify(settings) }),
  testProvider: ({ id, script }: Pick<GitPlatformProvider, 'id' | 'script'>) => aowRequest<{ name: string; hosts: string[]; operations: string[] }>('/api/aow/git-platform-providers/test', { method: 'POST', body: JSON.stringify({ id, script }) }),
};
