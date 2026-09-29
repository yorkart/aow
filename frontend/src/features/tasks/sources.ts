import { useEffect, useState } from 'react';
import { aowRequest } from '../../lib/aowRequest';
import { subscribeWorkspaceChanges } from '../../lib/workspaceEvents';
import { taskError } from './api';
import type { ReviewTarget } from '../pr/types';

export type RequirementSource = { type: 'inbox' } | { type: 'repository_issues'; enabled: boolean; provider: string | null; remote: string | null };
export interface SourceSettings { revision: number; sources: RequirementSource[] }
export interface SourceTargets { repository: string; targets: ReviewTarget[] }
export interface IssueLabel { name: string; color: string; description: string }
export interface RepositoryIssue { number: number; title: string; status: 'open' | 'closed'; url: string | null; labels: string[]; assignees: string[]; updated_at: string }
export interface IssueList { issues: RepositoryIssue[]; provider: string; provider_name: string; remote: string }

export const sourcePath = (projectId: string) => `/api/tasks/sources/${encodeURIComponent(projectId)}`;
export const saveSources = (projectId: string, settings: SourceSettings) => aowRequest<SourceSettings>(sourcePath(projectId), { method: 'PUT', body: JSON.stringify(settings) });

// The request identity includes filters and configuration. A late response from
// another project, remote or filter cannot replace the currently displayed data.
export function useSourceQuery<T>(path: string, enabled: boolean, refreshKey: string | number, body?: string, keepPrevious = false) {
  const key = JSON.stringify([path, refreshKey, body]);
  const request = JSON.stringify([path, body]);
  const [result, setResult] = useState<{ key: string; request: string; data?: T; error?: string; loading: boolean }>();
  useEffect(() => {
    if (!enabled) return;
    const controller = new AbortController();
    setResult(previous => ({ key, request, loading: true, data: keepPrevious && previous?.request === request ? previous.data : undefined }));
    void aowRequest<T>(path, { signal: controller.signal, ...(body === undefined ? {} : { method: 'POST', body }) })
      .then(data => { if (!controller.signal.aborted) setResult({ key, request, data, loading: false }); })
      .catch(error => { if (!controller.signal.aborted) setResult(previous => ({ ...previous, key, request, error: taskError(error), loading: false })); });
    return () => controller.abort();
  }, [path, key, request, body, enabled, keepPrevious]);
  return enabled && result && (result.key === key || (keepPrevious && result.request === request)) ? result : { loading: enabled, data: undefined, error: undefined };
}

export function useSourceChanges() {
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const update = () => setRevision(value => value + 1);
    const unsubscribe = subscribeWorkspaceChanges(change => { if (change.reset || change.tasks) update(); });
    window.addEventListener('aow-review-providers-changed', update);
    return () => { unsubscribe(); window.removeEventListener('aow-review-providers-changed', update); };
  }, []);
  return revision;
}
