import { useCallback } from 'react';
import { useAowTabLocationError, useAowTabNavigation } from '../../aow/AowTabEntry';
import { parseTabUrl } from '../../aow/tabRoutes';
import { fileRoute } from '../../aow/tabRoutes/files';
import { MarkdownContent } from '../../components/MarkdownContent';

function resolveFileLink(href: string, workspace: string, path: string) {
  const value = href.trim();
  if (!value || /^(?:[a-z][a-z\d+.-]*:|\/\/|[?#])/i.test(value)) return href;
  try {
    const base = new URL('https://markdown.invalid');
    // Encode the source path so directory names containing #, ? or % remain paths.
    base.pathname = path.split('/').map(encodeURIComponent).join('/');
    const target = new URL(value, base);
    if (target.origin !== base.origin) return href;
    const destination = decodeURIComponent(target.pathname);
    if (destination.includes('\0')) return href;
    return fileRoute.url({ type: 'file', workspace, path: destination, source: 'external' }) + target.hash;
  } catch {
    // Leave malformed URLs untouched instead of failing the whole preview.
    return href;
  }
}

export function MarkdownFileContent({ text, className, workspace, path }: {
  text: string; className: string; workspace: string; path: string;
}) {
  const navigateTab = useAowTabNavigation();
  const reportError = useAowTabLocationError();
  const resolveLink = useCallback((href: string) => resolveFileLink(href, workspace, path), [workspace, path]);
  return <MarkdownContent text={text} className={className} resolveLink={resolveLink} onOpenLink={href => {
    // Only rewritten local paths use in-app navigation; absolute web URLs keep
    // their native new-window behavior, even when they point at this server.
    if (!href.startsWith('/') || href.startsWith('//')) return false;
    const target = parseTabUrl(new URL(href, window.location.origin));
    if (target?.type !== 'file') return false;
    void navigateTab(target).catch(reason => reportError(reason instanceof Error ? reason.message : String(reason)));
    return true;
  }} />;
}
