import { parseTabUrl } from '../../aow/tabRoutes';
import { fileRoute } from '../../aow/tabRoutes/files';

export function localFileLink(href: string): { path: string; line?: number; hash: string } | undefined {
  const value = href.trim();
  if (!value || /^(?:[?#]|\/\/)/.test(value)) return;
  try {
    let destination = value;
    if (/^file:\/\//i.test(value)) {
      const url = new URL(value);
      if (url.hostname && url.hostname !== 'localhost') return;
      destination = `${url.pathname}${url.hash}`;
    }
    const hash = destination.includes('#') ? destination.slice(destination.indexOf('#')) : '';
    const line = /(?::(\d+)(?::\d+)?|#L?(\d+)(?:C\d+)?(?:-L?\d+)?)$/.exec(destination);
    const path = decodeURIComponent((line ? destination.slice(0, line.index) : destination).split(/[?#]/)[0]);
    if (!path || [...path].some(char => char.charCodeAt(0) < 32) || /^[a-z][a-z\d+.-]*:/i.test(path)) return;
    const number = line ? Number(line[1] || line[2]) : undefined;
    return { path, line: number && Number.isSafeInteger(number) ? number : undefined, hash };
  } catch { return; }
}

export function absoluteFilePath(path: string, cwd: string): string {
  const parts: string[] = [];
  for (const part of (path.startsWith('/') ? path : `${cwd}/${path}`).split('/')) {
    if (part === '..') parts.pop();
    else if (part && part !== '.') parts.push(part);
  }
  return `/${parts.join('/')}`;
}

export function fileTabLink(href: string) {
  if (!href.startsWith('/') || href.startsWith('//')) return;
  try {
    const url = new URL(href, window.location.origin);
    const target = parseTabUrl(url);
    if (target?.type === 'file') return { target, line: localFileLink(`/file${url.hash}`)?.line };
  } catch { /* A malformed Tab URL may still be a local file path. */ }
}

export function resolveMarkdownFileLink(href: string, workspace: string, directory: string) {
  // A resolved link can also come from authored HTML or the file:// renderer.
  if (fileTabLink(href)) return href;
  const file = localFileLink(href);
  if (!file) return href;
  return fileRoute.url({ type: 'file', workspace, path: absoluteFilePath(file.path, directory), source: 'external' })
    + (file.line ? `#L${file.line}` : file.hash);
}
