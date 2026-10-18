import { WorkspaceMarkdownContent } from './WorkspaceMarkdown';

export function MarkdownFileContent({ path, ...props }: {
  text: string; className: string; workspace: string; path: string;
}) {
  return <WorkspaceMarkdownContent {...props} directory={path.slice(0, path.lastIndexOf('/')) || '/'} />;
}
