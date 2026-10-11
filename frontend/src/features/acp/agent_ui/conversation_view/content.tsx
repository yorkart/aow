// ACP branches from thread_view.rs::render_output_content_block / render_tool_call.
import { lazy, memo, Suspense, useLayoutEffect, useRef, useState } from 'react';
import type { editor } from 'monaco-editor';
import { FileText } from 'lucide-react';
import { AgentMarkdown } from './markdown';
import { object, text, type Data } from '../../types';
import { localFileLink } from './file_links';

export type OpenFile = (path: string, line?: number) => void;
export const ContentBlock = memo(function ContentBlock({ content, onOpenFile, user = false, message = false }: { content: Data; onOpenFile: OpenFile; user?: boolean; message?: boolean }) {
  if (content.type === 'text') return user ? <div className="acp-user-text conversation-prose">{text(content.text)}</div> : <AgentMarkdown text={text(content.text)} onOpenFile={onOpenFile} message={message} />;
  if (content.type === 'image' && /^image\/(png|jpeg|webp|gif)$/.test(text(content.mimeType))) return <img className="acp-image" alt={user ? 'User image' : 'Agent image'} src={`data:${text(content.mimeType)};base64,${text(content.data)}`} />;
  if (content.type === 'resource') return <span className="acp-resource-label">{text(object(content.resource).uri)}</span>;
  if (content.type === 'resource_link') return <ResourceLink uri={text(content.uri)} onOpenFile={onOpenFile} />;
  // Upstream has no output renderer for audio or unknown ACP content variants.
  return null;
});
function ResourceLink({ uri, onOpenFile }: { uri: string; onOpenFile: OpenFile }) {
  const file = /^file:\/\//i.test(uri) ? localFileLink(uri) : undefined;
  if (file) return <button type="button" className="acp-file-link" onClick={() => onOpenFile(file.path, file.line)}><FileText size={13} />{uri.slice(7)}</button>;
  if (/^https?:\/\//.test(uri)) return <a className="acp-resource-link" href={uri} target="_blank" rel="noreferrer">{uri}</a>;
  return <span className="acp-resource-label">{uri}</span>;
}
const DiffEditor = lazy(() => import('../../../editor/MonacoEditor').then(module => ({ default: module.DiffEditor })));
export function DiffContent({ content, onOpenFile }: { content: Data; onOpenFile: OpenFile }) {
  const host = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(90);
  const instance = useRef<editor.IStandaloneDiffEditor | undefined>(undefined);
  useLayoutEffect(() => () => {
    // Detach before @monaco-editor/react disposes models during its passive cleanup.
    // Otherwise removing a streaming permission preview races Monaco's diff worker.
    const models = instance.current?.getModel();
    instance.current?.setModel(null);
    models?.original.dispose(); models?.modified.dispose();
    instance.current = undefined;
  }, []);
  return <div className="acp-diff acp-code" ref={host}><button type="button" className="acp-file-link" onClick={() => onOpenFile(text(content.path))}><FileText size={13} />{text(content.path)}</button>
    <Suspense fallback={<p className="acp-muted">正在加载变更…</p>}><DiffEditor onMount={mounted => {
      instance.current = mounted;
      const updateFont = () => {
        if (!host.current) return;
        const style = getComputedStyle(host.current);
        mounted.updateOptions({ fontSize: parseFloat(style.fontSize), fontFamily: style.fontFamily, lineHeight: parseFloat(style.lineHeight) });
      };
      const observer = new ResizeObserver(updateFont);
      observer.observe(host.current!);
      mounted.onDidDispose(() => observer.disconnect());
      updateFont();
      const resize = () => setHeight(Math.max(60, mounted.getModifiedEditor().getContentHeight()));
      mounted.getModifiedEditor().onDidContentSizeChange(resize); resize();
    }} original={text(content.oldText)} modified={text(content.newText)} theme="vs-dark" height={height} options={{ readOnly: true, renderSideBySide: false, minimap: { enabled: false }, scrollBeyondLastLine: false, automaticLayout: true, lineNumbers: 'off', glyphMargin: false, folding: false, renderOverviewRuler: false, overviewRulerLanes: 0, scrollBeyondLastColumn: 0, scrollbar: { vertical: 'hidden', alwaysConsumeMouseWheel: false }, hideUnchangedRegions: { enabled: true }, padding: { top: 6, bottom: 6 } }} /></Suspense>
  </div>;
}
