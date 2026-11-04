import type { MyPullRequests, PullRequestDetail, PullRequestDiff } from './types';
import type { GitPlatformIdentity } from '../git-platform-providers/types';
import { request } from '../../lib/http';

function reviewQuery(repo: string, target?: GitPlatformIdentity) {
  return new URLSearchParams({ repo, ...(target?.provider ? { provider: target.provider } : {}), ...(target?.remote ? { remote: target.remote } : {}) });
}

// A paginated provider call can run for 60 seconds on the server. Do not retry
// scripts automatically: the adapter owns the implementation of its API.
function reviewRequest<T>(url: string) { return request<T>(url, undefined, 0, 65_000); }

export const prApi = {
  myPullRequests: (repo: string, target?: GitPlatformIdentity, state?: 'open' | 'all') => reviewRequest<MyPullRequests>('/api/my-pull-requests?' + new URLSearchParams({ ...Object.fromEntries(reviewQuery(repo, target)), ...(state ? { state } : {}) })),
  myPullRequest: (repo: string, number: number, target?: GitPlatformIdentity) => reviewRequest<PullRequestDetail>('/api/my-pull-requests/' + number + '?' + reviewQuery(repo, target)),
  myPullRequestDiff: (repo: string, number: number, path: string, patchOnly = false, target?: GitPlatformIdentity) => reviewRequest<PullRequestDiff>('/api/my-pull-requests/' + number + '/diff?' + new URLSearchParams({ ...Object.fromEntries(reviewQuery(repo, target)), path, ...(patchOnly ? { patch_only: 'true' } : {}) })),
};
