import { useEffect, useRef, useState, type PointerEvent, type KeyboardEvent } from 'react';
import { GripVertical, LoaderCircle, MessageSquare, Play, Trash2 } from 'lucide-react';
import type { AowProject } from '../../aow/types';
import type { AowAgent } from '../agents/types';
import { AgentIcon } from '../agents/AgentIcon';
import { aowAgentType } from '../agents/agentTypes';
import { MarkdownContent } from '../../components/MarkdownContent';
import { inboxApi } from './api';
import { newInboxId } from './id';
import { InboxExecutePanel } from './InboxExecutePanel';
import { InboxComments } from './InboxComments';
import { InlineEditor } from './InlineEditor';
import { EditorShortcutHint } from './EditorShortcutHint';
import { RowLabels } from './RowLabels';
import { ProjectSelect } from './ProjectSelect';
import { saveDraft } from './drafts';
import { useInboxDraft } from './useInboxDraft';
import { useEditorDismiss } from './useEditorDismiss';
import { inboxError, previewTitle, type InboxExecuteOptions, type InboxExecution, type InboxItem, type InboxLabel } from './types';

export function InboxRow({ item, labels, projects, agents, execution, commentCount, mutate, onOpenTerminal, onEditingChange, dragProps, dragging, drop }: {
  item: InboxItem; labels: InboxLabel[]; projects: AowProject[]; agents: AowAgent[]; execution?: InboxExecution;
  commentCount: number;
  mutate: <T>(action: () => Promise<T>) => Promise<T>; onOpenTerminal: (id: string) => Promise<void>;
  onEditingChange: (id: string, editing: boolean) => void;
  dragging: boolean; drop?: 'before' | 'after';
  dragProps: { onPointerDown: (e: PointerEvent<HTMLButtonElement>) => void; onPointerMove: (e: PointerEvent<HTMLButtonElement>) => void; onPointerUp: (e: PointerEvent<HTMLButtonElement>) => void; onPointerCancel: () => void; onKeyDown: (e: KeyboardEvent<HTMLButtonElement>) => void };
}) {
  const editor = useInboxDraft(item.id, mutate, item);
  const draft = editor.draft;
  const root = useRef<HTMLElement>(null);
  useEditorDismiss(root, editor.editing, focus => { if (focus) finishEdit(); else void editor.finish(); });
  useEffect(() => { onEditingChange(item.id, !!draft); }, [item.id, !!draft, onEditingChange]);
  useEffect(() => () => onEditingChange(item.id, false), [item.id, onEditingChange]);
  const [pendingLabels, setPendingLabels] = useState<string[]>();
  const [section, setSection] = useState<'delete'>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [configureExecution, setConfigureExecution] = useState(false);
  const [showComments, setShowComments] = useState(false);
  const content = useRef<HTMLDivElement>(null);
  const [editorHeight, setEditorHeight] = useState<number>();
  const executionRequest = useRef<{ key: string; revision: number; signature: string } | undefined>(undefined);
  const action = async (operation: () => Promise<unknown>, done?: () => void) => {
    setBusy(true); setError('');
    try { await mutate(operation); done?.(); } catch (reason) { setError(inboxError(reason)); }
    finally { setBusy(false); }
  };
  const beginEdit = () => {
    if (busy || editor.editing || editor.removing) return;
    setEditorHeight(content.current?.getBoundingClientRect().height);
    editor.begin();
    setSection(undefined); setError('');
  };
  const finishEdit = () => { void editor.finish(); requestAnimationFrame(() => content.current?.querySelector<HTMLElement>('.inbox-edit-target')?.focus({ preventScroll: true })); };
  const projectExists = projects.some(project => project.id === item.project_id);
  const boundProject = projects.find(project => project.id === item.project_id);
  const panelContainer = configureExecution ? root.current?.closest<HTMLElement>('.inbox-panel') : null;
  const activeLabels = draft ? draft.label_ids : pendingLabels ?? item.label_ids;
  const starting = execution?.phase === 'starting';
  const executedAgent = agents.find(agent => agent.id === execution?.agent);
  const terminalTitle = `打开 ${executedAgent?.display_name || execution?.agent} 终端`;
  const toggle = (next: typeof section) => { setSection(value => value === next ? undefined : next); setError(''); };
  const execute = (options: InboxExecuteOptions) => {
    if (busy || starting) return;
    const signature = JSON.stringify(options);
    const currentRequest = executionRequest.current;
    const request = currentRequest && currentRequest.revision === item.revision && currentRequest.signature === signature
      ? currentRequest
      : { key: newInboxId(), revision: item.revision, signature };
    executionRequest.current = request;
    void action(() => inboxApi.execute(item, options, request.key), () => {
      executionRequest.current = undefined;
      setConfigureExecution(false);
    });
  };
  return <article ref={root} className={`inbox-row${dragging ? ' dragging' : ''}${editor.editing ? ' editing' : ''}`} data-inbox-id={item.id} data-inbox-editor={item.id} data-drop={drop} aria-label={previewTitle(item.markdown)}>
    <button className="inbox-grip" aria-label={`拖动排序：${previewTitle(item.markdown)}`} title="拖动排序，或使用上下方向键" disabled={busy || !!draft} {...dragProps}><GripVertical size={16} /></button>
    <div className="inbox-row-body">
      <div ref={content} className="inbox-content">
        {draft && editor.editing ? <InlineEditor seamless initialHeight={editorHeight} value={draft.markdown} onChange={markdown => editor.update({ markdown })}
          onFinish={() => { void finishEdit(); }} onComposing={editor.setComposing} status={editor.status} />
          : <div className="inbox-edit-target" role="group" tabIndex={busy ? -1 : 0} aria-label="需求内容，双击或按 Enter 编辑" aria-keyshortcuts="Enter F2" title="双击编辑"
            onDoubleClick={event => { if (!(event.target as Element).closest('a, button, input, select, textarea, summary')) beginEdit(); }}
            onKeyDown={event => { if (event.target === event.currentTarget && (event.key === 'Enter' || event.key === 'F2')) { event.preventDefault(); beginEdit(); } }}>
            <MarkdownContent text={draft?.markdown ?? item.markdown} readOnly className="inbox-markdown" />
          </div>}
      </div>
      {draft && editor.conflict && <p className="inbox-conflict">这条需求已有更新。<button disabled={editor.saving} onClick={() => editor.resolve(true)}>{editor.removing ? '按最新版本移除' : '保留草稿，以最新版本保存'}</button><button disabled={editor.saving} onClick={() => editor.resolve(false)}>载入最新内容</button></p>}
      <div className="inbox-row-meta">
        <ProjectSelect projects={projects} value={(draft ? draft.project_id : item.project_id) ?? ''} disabled={busy || editor.removing} onChange={value => {
          const project_id = value || null;
          if (draft) editor.update({ project_id });
          else void action(() => inboxApi.update(item, { project_id }));
        }} />
        <RowLabels labels={labels} selected={activeLabels} busy={busy || editor.removing} onChange={next => {
          if (draft) { editor.update({ label_ids: next }); return; }
          setPendingLabels(next);
          void action(() => inboxApi.update(item, { label_ids: next })).finally(() => setPendingLabels(undefined));
        }} />
        <div className="inbox-row-actions" aria-busy={starting}>
          {execution?.tab_id && <button type="button" aria-label="打开终端" title={terminalTitle} onClick={() => {
            setError('');
            void onOpenTerminal(execution.tab_id!).catch(reason => setError(inboxError(reason)));
          }}><AgentIcon agentId={executedAgent ? aowAgentType(executedAgent) : execution.agent} /></button>}
          {commentCount > 0 && <button type="button" aria-label="评论" title={showComments ? '收起评论' : '展开评论'}
            aria-expanded={showComments} aria-controls={`inbox-comments-${item.id}`} className={showComments ? 'active' : undefined}
            onClick={() => setShowComments(value => !value)}><MessageSquare size={14} /></button>}
          {draft ? <span className="inbox-save-status" role="status">{editor.status}</span> : <>
            <button type="button" aria-label="执行需求" title={starting ? '正在启动 Agent…' : !projectExists ? '先绑定项目再执行' : '配置执行并选择 Agent'} disabled={busy || !projectExists || !!starting}
              onClick={() => { setError(''); setConfigureExecution(true); }}>
              {starting ? <LoaderCircle size={14} className="inbox-spin" /> : <Play size={14} />}
            </button>
            <button aria-label="删除需求" title="删除需求" disabled={busy} onClick={() => toggle('delete')}><Trash2 size={14} /></button>
          </>}
          {editor.editing && <EditorShortcutHint />}
        </div>
      </div>
      {showComments && commentCount > 0 && <InboxComments itemId={item.id} count={commentCount} />}
      {section === 'delete' && <div className="inbox-delete"><span>删除这条需求？</span><button disabled={busy} onClick={() => setSection(undefined)}>取消</button><button className="inbox-danger" disabled={busy} onClick={() => void action(() => inboxApi.remove(item), () => saveDraft(item.id))}>确认删除</button></div>}
      {execution && (execution.phase === 'failed' || execution.phase === 'interrupted') && <div className={`inbox-execution ${execution.phase}`} role="alert">
        {execution.error || '执行未完成'}
      </div>}
      {error && !configureExecution && <p className="inbox-error" role="alert">{error}</p>}
      {draft && editor.error && <p className="inbox-error" role="alert">{editor.error}<button onClick={() => { void editor.retry(); }}>重试</button></p>}
    </div>
    {configureExecution && boundProject && panelContainer && <InboxExecutePanel container={panelContainer} item={item} project={boundProject} agents={agents} busy={busy} error={error} onClose={() => setConfigureExecution(false)} onSubmit={execute} />}
  </article>;
}
