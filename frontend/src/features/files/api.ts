import type { DirectoryListing, TextFile, PinnedDirectories } from './types';
import { fetchWithTimeout, HttpRequestError, request } from '../../lib/http';
import { aowRequest } from '../../lib/aowRequest';
import { appUrl } from '../../lib/basePath';

const encodePath = (path: string) => path.split('/').map(encodeURIComponent).join('/');

async function ensureFile(path: string) {
  // The raw-file endpoint validates that the target is a readable regular
  // file.  Cancel the body immediately so this remains a metadata check.
  const response = await fetchWithTimeout(`/api/fs/raw${encodePath(path)}`, {
    cache: 'no-store',
  });
  if (!response.ok) {
    const payload = await response.json().catch(() => null);
    throw new HttpRequestError(payload?.message ?? payload?.error?.message ?? `HTTP ${response.status}`);
  }
  await response.body?.cancel();
}

export const filesApi = {
  // `/api/fs/tree` is the root-directory route.  Keeping the root request
  // free of a trailing slash avoids falling through to the SPA route.
  listDirectory: (path: string) => request<DirectoryListing>(path === '/' ? '/api/fs/tree' : `/api/fs/tree${encodePath(path)}`),
  listHomeDirectory: () => request<DirectoryListing>('/api/fs/home'),
  ensureFile,
  readText: (path: string, signal?: AbortSignal) => request<TextFile>(`/api/fs/text${encodePath(path)}`, { cache: 'no-store', signal }),
  rawUrl: (path: string) => appUrl(`/api/fs/raw${encodePath(path)}`),
  readBlob: async (path: string, signal?: AbortSignal) => {
    const response = await fetchWithTimeout(`/api/fs/raw${encodePath(path)}`, { cache: 'no-store', signal }, 30_000);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.blob();
  },
  readBinary: async (path: string) => {
    const response = await fetchWithTimeout(`/api/fs/raw${encodePath(path)}`, { cache: 'no-store' }, 30_000);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.arrayBuffer();
  },
  saveText: async (path: string, content: string, version?: string) => {
    const response = await fetchWithTimeout(`/api/fs/file${encodePath(path)}`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'text/plain; charset=utf-8',
        ...(version ? { 'If-Match': `"${version}"` } : {}),
      },
      body: content,
    }, 20_000);
    const payload = await response.json().catch(() => null);
    if (!response.ok) throw new Error(payload?.message ?? `HTTP ${response.status}`);
    return payload as { path: string; size: number; created: boolean; version: string };
  },
  createEntry: (parent: string, name: string, kind: 'file' | 'directory') => request<{ path: string; name: string; kind: 'file' | 'directory' }>('/api/fs/entries', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ parent, name, kind }),
  }, 0),
  renameEntry: (path: string, name: string) => request<{ path: string; name: string }>('/api/fs/entries', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path, name }),
  }, 0),
  deleteEntry: (path: string) => request<void>(`/api/fs/entries?${new URLSearchParams({ path })}`, { method: 'DELETE' }, 0),
  uploadFile: (directory: string, file: File, onProgress?: (loaded: number) => void, options?: { keepBoth?: boolean; signal?: AbortSignal }) => {
    const path = `${directory.replace(/\/$/, '')}/${file.name}`;
    return new Promise<{ path: string; size: number; created: boolean; version: string }>((resolve, reject) => {
      const request = new XMLHttpRequest();
      request.open('PUT', appUrl(`/api/fs/file${encodePath(path)}${options?.keepBoth ? '?keep_both=true' : ''}`));
      request.setRequestHeader('Content-Type', file.type || 'application/octet-stream');
      request.responseType = 'json';
      request.timeout = 120_000;
      request.upload.onprogress = (event) => onProgress?.(event.loaded);
      request.onload = () => {
        if (request.status >= 200 && request.status < 300) resolve(request.response);
        else reject(new Error(request.response?.message ?? `HTTP ${request.status}`));
      };
      request.onerror = () => reject(new Error('上传失败，请检查网络连接'));
      request.ontimeout = () => reject(new Error('上传超时（120 秒）'));
      request.onabort = () => reject(new Error('上传已取消'));
      const abort = () => request.abort();
      request.onloadend = () => options?.signal?.removeEventListener('abort', abort);
      if (options?.signal?.aborted) { reject(new Error('上传已取消')); return; }
      options?.signal?.addEventListener('abort', abort, { once: true });
      request.send(file);
    });
  },
  pinnedDirectories: () => aowRequest<PinnedDirectories>('/api/aow/pinned-directories', { cache: 'no-store' }),
  updatePinnedDirectories: (input: { add?: string[]; remove?: string[] }) => aowRequest<PinnedDirectories>('/api/aow/pinned-directories', {
    method: 'PATCH', body: JSON.stringify(input),
  }),
};
