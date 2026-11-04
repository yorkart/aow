import { createContext, useCallback, useContext, useMemo, type ComponentProps, type ReactNode } from 'react';
import { useAowTabLocationError, useAowTabNavigation } from '../../aow/AowTabEntry';
import { MarkdownContent } from '../../components/MarkdownContent';
import { MarkdownImagePreview } from './MarkdownImagePreview';
import { fileTabLink, localFileLink, resolveMarkdownFileLink } from './markdownFileLinks';

type MarkdownProps = Omit<ComponentProps<typeof MarkdownContent>, 'resolveLink' | 'onOpenLink'> & {
  onOpenFile?: (path: string, line?: number) => void;
};
type Workspace = { workspace: string; directory: string };
const MarkdownWorkspace = createContext<Workspace | undefined>(undefined);

export function WorkspaceMarkdownProvider({ workspace, directory, enabled = true, children }: {
  workspace?: string; directory?: string; enabled?: boolean; children: ReactNode;
}) {
  const parent = useContext(MarkdownWorkspace);
  const id = workspace ?? parent?.workspace;
  const cwd = directory || parent?.directory;
  const value = useMemo(() => enabled && id && cwd ? { workspace: id, directory: cwd } : undefined, [enabled, id, cwd]);
  return <MarkdownWorkspace.Provider value={value}>{children}</MarkdownWorkspace.Provider>;
}

export function WorkspaceMarkdownContent({ workspace, directory, onOpenFile, ...props }: MarkdownProps & Workspace) {
  const navigateTab = useAowTabNavigation();
  const reportError = useAowTabLocationError();
  const resolveLink = useCallback((href: string) => resolveMarkdownFileLink(href, workspace, directory), [workspace, directory]);
  return <MarkdownImagePreview key={`${workspace}:${directory}`} content={props.text}>
    <MarkdownContent {...props} resolveLink={resolveLink} onOpenLink={href => {
      const file = fileTabLink(href);
      if (!file) return false;
      if (onOpenFile && file.target.workspace === workspace) onOpenFile(file.target.path, file.line);
      else void navigateTab(file.target).catch(reason => reportError(reason instanceof Error ? reason.message : String(reason)));
      return true;
    }} />
  </MarkdownImagePreview>;
}

export function ContextualMarkdownContent({ onOpenFile, ...props }: MarkdownProps) {
  const workspace = useContext(MarkdownWorkspace);
  if (workspace) return <WorkspaceMarkdownContent {...props} {...workspace} onOpenFile={onOpenFile} />;
  return <MarkdownContent {...props} onOpenLink={onOpenFile ? href => {
    const file = localFileLink(href);
    if (!file) return false;
    onOpenFile(file.path, file.line);
    return true;
  } : undefined} />;
}
