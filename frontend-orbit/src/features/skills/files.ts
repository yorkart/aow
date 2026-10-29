import type { SkillEntry, SkillFile } from './types';

export function validPath(path: string): boolean {
  return Boolean(path) && path.length <= 240 && !/[\\\p{Cc}]/u.test(path) && path.split('/').every(part => part.trim() && part !== '.' && part !== '..');
}

export function withParents(files: SkillEntry[]): SkillEntry[] {
  const result = new Map(files.map(file => [file.path, file]));
  for (const file of files) {
    const parts = file.path.split('/'); parts.pop();
    while (parts.length) {
      const path = parts.join('/');
      if (result.get(path)?.kind === 'file') throw new Error(`文件不能同时作为目录：${path}`);
      if (!result.has(path)) result.set(path, { path, kind: 'directory' });
      parts.pop();
    }
  }
  return [...result.values()].sort((a, b) => a.path.localeCompare(b.path));
}

export function base64(buffer: Uint8Array): string {
  let content = '';
  for (let i = 0; i < buffer.length; i += 16_384) content += String.fromCharCode(...buffer.subarray(i, i + 16_384));
  return btoa(content);
}

export function mime(path: string) {
  const extension = path.split('.').pop()?.toLowerCase() ?? '';
  return ({ png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp', svg: 'image/svg+xml', avif: 'image/avif', bmp: 'image/bmp' } as Record<string, string>)[extension] ?? 'application/octet-stream';
}

export function dataUrl(file: SkillFile) { return `data:${mime(file.path)};base64,${file.encoding === 'base64' ? file.content : base64(new TextEncoder().encode(file.content))}`; }

export function download(file: SkillFile) {
  const content = file.encoding === 'base64' ? Uint8Array.from(atob(file.content), char => char.charCodeAt(0)) : new TextEncoder().encode(file.content);
  const url = URL.createObjectURL(new Blob([content], { type: 'application/octet-stream' }));
  const anchor = document.createElement('a'); anchor.href = url; anchor.download = file.path.split('/').pop()!; anchor.click();
  window.setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function relativePath(current: string, href: string): string | undefined {
  if (/^[a-z][a-z0-9+.-]*:|^\/\//i.test(href) || href.startsWith('#')) return undefined;
  let path: string;
  try { path = decodeURIComponent(href.split(/[?#]/)[0]); } catch { return undefined; }
  if (path.startsWith('/') || path.includes('\\')) return undefined;
  const parts = current.split('/'); parts.pop();
  for (const part of path.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') { if (!parts.length) return undefined; parts.pop(); }
    else parts.push(part);
  }
  return parts.join('/');
}
