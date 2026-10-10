// ACP branches from thread_view.rs::render_output_content_block / render_tool_call.
import { lazy, Suspense, useLayoutEffect, useRef, useState } from 'react';
import type { editor } from 'monaco-editor';
import { Check, ChevronDown, ChevronRight, FileText, LoaderCircle, Terminal, Wrench, X } from 'lucide-react';
import { MarkdownContent } from '../../../../components/MarkdownContent';
import { list, object, text, type Data, type Permission } from '../../types';
import { PermissionRequest } from './elicitation';
import { toolContent } from './entry_view_state';
import { TerminalOutput } from './terminal_output';
import { localFileLink } from './file_links';

export type OpenFile = (path: string, line?: number) => void;
export function ContentBlock({ content, onOpenFile, user = false }: { content: Data; onOpenFile: OpenFile; user?: boolean }) {
  if (content.type === 'text') return user ? <div className="zed-user-text">{text(content.text)}</div> : <MarkdownContent text={text(content.text)} readOnly onOpenLink={href => { const file = localFileLink(href); if (!file) return false; onOpenFile(file.path, file.line); return true; }} />;
  if (content.type === 'image' && /^image\/(png|jpeg|webp|gif)$/.test(text(content.mimeType))) return <img className="zed-image" alt="Agent image" src={`data:${text(content.mimeType)};base64,${text(content.data)}`} />;
  if (content.type === 'resource') return <span className="zed-resource-label">{text(object(content.resource).uri)}</span>;
  if (content.type === 'resource_link') return <ResourceLink uri={text(content.uri)} onOpenFile={onOpenFile} />;
  // Zed has no output renderer for audio or unknown ACP content variants.
  return null;
}
function ResourceLink({ uri, onOpenFile }: { uri: string; onOpenFile: OpenFile }) {
  const file = /^file:\/\//i.test(uri) ? localFileLink(uri) : undefined;
  if (file) return <button type="button" className="zed-file-link" onClick={() => onOpenFile(file.path, file.line)}><FileText size={13} />{uri.slice(7)}</button>;
  if (/^https?:\/\//.test(uri)) return <a className="zed-resource-link" href={uri} target="_blank" rel="noreferrer">{uri}</a>;
  return <span className="zed-resource-label">{uri}</span>;
}
const DiffEditor = lazy(() => import('../../../editor/MonacoEditor').then(module => ({ default: module.DiffEditor })));
function DiffContent({ content, onOpenFile }: { content: Data; onOpenFile: OpenFile }) {
  const instance = useRef<editor.IStandaloneDiffEditor | undefined>(undefined);
  useLayoutEffect(() => () => {
    // Detach before @monaco-editor/react disposes models during its passive cleanup.
    // Otherwise removing a streaming permission preview races Monaco's diff worker.
    const models = instance.current?.getModel();
    instance.current?.setModel(null);
    models?.original.dispose(); models?.modified.dispose();
    instance.current = undefined;
  }, []);
  return <div className="zed-diff"><button type="button" className="zed-file-link" onClick={() => onOpenFile(text(content.path))}><FileText size={13} />{text(content.path)}</button>
    <Suspense fallback={<p className="zed-muted">正在加载变更…</p>}><DiffEditor onMount={mounted => { instance.current = mounted; }} original={text(content.oldText)} modified={text(content.newText)} theme="vs-dark" height={Math.min(320, Math.max(90, text(content.newText).split('\n').length * 20 + 30))} options={{ readOnly: true, renderSideBySide: false, minimap: { enabled: false }, scrollBeyondLastLine: false, automaticLayout: true, fontSize: 12 }} /></Suspense>
  </div>;
}
export function ToolCall({ content, terminals, onOpenFile, permission, busy = false, onAnswer }: { content: Data; terminals?: Record<string, Data>; onOpenFile: OpenFile; permission?: Permission; busy?: boolean; onAnswer?: (response: Data) => void }) {
  const [expanded, setExpanded] = useState(false);
  const parts = toolContent(content); const locations = list(content.locations);
  const edit = content.kind === 'edit' || parts.some(part => part.type === 'diff');
  const execute = content.kind === 'execute';
  const rawInput = !edit && !execute && !parts.some(part => object(part.content).type === 'image') && content.rawInput != null;
  const open = expanded || !!permission;
  const pending = ['pending', 'in_progress'].includes(text(content.status));
  const failed = ['failed', 'rejected', 'cancelled', 'canceled'].includes(text(content.status));
  const card = edit || execute || !!permission;
  const location = (value: Data, index = 0) => <button className="zed-file-link" type="button" key={index} onClick={() => onOpenFile(text(value.path), typeof value.line === 'number' ? value.line : undefined)}>{text(value.path)}{typeof value.line === 'number' ? `:${value.line}` : ''}</button>;
  return <section className={`zed-tool${card ? ' zed-tool-card' : ''}${failed ? ' zed-tool-failed' : ''}`} aria-label={text(content.title) || 'Tool'}>
    <div className="zed-tool-header"><button type="button" className="zed-tool-toggle" aria-expanded={open} disabled={!parts.length && !rawInput && locations.length < 2 || !!permission} onClick={() => setExpanded(value => !value)}>
      {pending ? <LoaderCircle size={14} className="zed-spinner" /> : failed ? <X size={14} /> : execute ? <Terminal size={14} /> : edit ? <FileText size={14} /> : content.status === 'completed' ? <Check size={14} /> : <Wrench size={14} />}
      <span>{text(content.title) || text(content.kind) || 'Tool'}</span>{(parts.length > 0 || rawInput) && (open ? <ChevronDown size={12} /> : <ChevronRight size={12} />)}
    </button>{locations.length === 1 && location(locations[0])}</div>
    {open && <div className="zed-tool-output">{locations.length > 1 && locations.map(location)}
      {rawInput && <details className="zed-tool-input"><summary>输入参数</summary><pre>{JSON.stringify(content.rawInput, null, 2)}</pre></details>}
      {parts.map((part, index) => part.type === 'content' ? <ContentBlock key={index} content={object(part.content)} onOpenFile={onOpenFile} /> : part.type === 'diff' ? <DiffContent key={index} content={part} onOpenFile={onOpenFile} /> : part.type === 'terminal' ? <TerminalOutput key={index} terminal={terminals?.[text(part.terminalId)]} pending={pending} /> : null)}
    </div>}
    {permission && onAnswer && <PermissionRequest permission={permission} busy={busy} onAnswer={onAnswer} />}
  </section>;
}
