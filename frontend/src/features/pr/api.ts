import type { ReviewIdentity, ReviewTarget, MyPullRequests, PullRequestDetail, PullRequestDiff, ReviewProvider, ReviewProviderSettings } from './types';
import { aowRequest } from '../../lib/aowRequest';
import { request } from '../../lib/http';

function reviewQuery(repo: string, target?: ReviewIdentity) {
  return new URLSearchParams({ repo, ...(target?.provider ? { provider: target.provider } : {}), ...(target?.remote ? { remote: target.remote } : {}) });
}

// A paginated provider call can run for 60 seconds on the server. Do not retry
// scripts automatically: the adapter owns the implementation of its API.
function reviewRequest<T>(url: string) { return request<T>(url, undefined, 0, 65_000); }

export const prApi = {
  reviewTargets: (repo: string) => reviewRequest<ReviewTarget[]>('/api/review-targets?repo=' + encodeURIComponent(repo)),
  myPullRequests: (repo: string, target?: ReviewIdentity) => reviewRequest<MyPullRequests>('/api/my-pull-requests?' + reviewQuery(repo, target)),
  myPullRequest: (repo: string, number: number, target?: ReviewIdentity) => reviewRequest<PullRequestDetail>('/api/my-pull-requests/' + number + '?' + reviewQuery(repo, target)),
  myPullRequestDiff: (repo: string, number: number, path: string, patchOnly = false, target?: ReviewIdentity) => reviewRequest<PullRequestDiff>('/api/my-pull-requests/' + number + '/diff?' + new URLSearchParams({ ...Object.fromEntries(reviewQuery(repo, target)), path, ...(patchOnly ? { patch_only: 'true' } : {}) })),
  reviewProviders: () => aowRequest<ReviewProviderSettings>('/api/aow/review-providers'),
  saveReviewProviders: (settings: ReviewProviderSettings) => aowRequest<ReviewProviderSettings>('/api/aow/review-providers', { method: 'PUT', body: JSON.stringify(settings) }),
  testReviewProvider: (provider: ReviewProvider) => aowRequest<{ operations: string[] }>('/api/aow/review-providers/test', { method: 'POST', body: JSON.stringify(provider) }),
};
