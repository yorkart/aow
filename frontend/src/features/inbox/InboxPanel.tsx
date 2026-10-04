import { useCallback, useRef, useState } from 'react';
import { Inbox, Plus } from 'lucide-react';
import type { AowProject } from '../../aow/types';
import type { AowAgent } from '../agents/types';
import { inboxApi } from './api';
import { InboxRow } from './InboxRow';
import { InlineEditor } from './InlineEditor';
import { InboxToolbar } from './InboxToolbar';
import { MarkdownContent } from '../../components/MarkdownContent';
import { useInboxDraft } from './useInboxDraft';
import { useEditorDismiss } from './useEditorDismiss';
import { useInbox } from './useInbox';
import { inboxError } from './types';
import './inbox.css';
interface Drag { id: string; over?: string; after: boolean; x: number; y: number; moved: boolean }
export function InboxPanel({ visible, projects, agents, onOpenTerminal }: {
  visible: boolean; projects: AowProject[]; agents: AowAgent[]; onOpenTerminal: (id: string) => Promise<void>;
}) {
  const { data, error, reload, mutate } = useInbox(visible);
  const capture = useInboxDraft('new', mutate, undefined, data?.items);
  const draft = capture.draft;
  const [editingIds, setEditingIds] = useState<Set<string>>(() => new Set());
  const onEditingChange = useCallback((id: string, editing: boolean) => setEditingIds(current => {
    if (current.has(id) === editing) return current;
    const next = new Set(current); if (editing) next.add(id); else next.delete(id); return next;
  }), []);
  const [project, setProject] = useState('');
  const [labels, setLabels] = useState<string[]>([]);
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const [actionError, setActionError] = useState('');
  const drag = useRef<Drag | undefined>(undefined);
  const [dragging, setDragging] = useState<Drag>();
  const editor = useRef<HTMLDivElement>(null);
  useEditorDismiss(editor, capture.editing, () => { void capture.finish(); });
  const ownProjects = projects.filter(project => !project.builtin);
  const selectedLabels = labels.filter(id => data?.labels.some(label => label.id === id));
  const items = (data?.items ?? []).filter(item => item.id !== draft?.base?.id && (editingIds.has(item.id) || ((!project || (project === 'unbound' ? !item.project_id : item.project_id === project))
    && (!selectedLabels.length || item.label_ids.some(id => selectedLabels.includes(id))) && item.markdown.toLowerCase().includes(query.toLowerCase()))));
  const add = async () => {
    setProject(''); setLabels([]); setQuery('');
    if (draft && !await capture.finish()) return;
    capture.begin();
    requestAnimationFrame(() => { editor.current?.scrollIntoView({ block: 'nearest' }); editor.current?.querySelector<HTMLElement>('[role="textbox"]')?.focus(); });
  };
  const move = async (id: string, before: string | null) => {
    if (!data || busy || before === id) return;
    setBusy(true); setActionError('');
    try { await mutate(() => inboxApi.reorder(data.revision, id, before)); }
    catch (reason) { setActionError(inboxError(reason)); }
    finally { setBusy(false); }
  };
  return <section className="inbox-panel" aria-label="Inbox">
    <InboxToolbar visible={visible} data={data} projects={ownProjects} project={project} query={query} labels={selectedLabels} count={items.length}
      onProject={setProject} onQuery={setQuery} onLabel={id => setLabels(current => current.includes(id) ? current.filter(value => value !== id) : [...current, id])}
      onClear={() => { setProject(''); setLabels([]); setQuery(''); }} mutate={mutate} />
    <div className="inbox-scroll">
      {error && <div className="inbox-error" role="alert">{error}<button onClick={() => void reload()}>重试加载</button></div>}
      {!data && !error && <p className="inbox-empty">正在加载…</p>}
      {data && !items.length && !draft && <div className="inbox-empty"><Inbox size={28} /><strong>{data.items.length ? '没有匹配的需求' : '先把想法记下来'}</strong><p>{data.items.length ? '调整筛选，或添加新的需求。' : '一段 Markdown 就够了，项目和标签可以稍后补充。'}</p></div>}
      <div className="inbox-list" aria-label="需求列表">{items.map((item, index) => <InboxRow key={item.id} item={item} labels={data!.labels} projects={ownProjects} agents={agents} execution={data?.executions.find(run => run.item_id === item.id)}
        commentCount={data?.comment_counts[item.id] ?? 0}
        mutate={mutate} onOpenTerminal={onOpenTerminal} onEditingChange={onEditingChange} dragging={dragging?.id === item.id} drop={dragging?.over === item.id ? dragging.after ? 'after' : 'before' : undefined}
        dragProps={{
          onPointerDown: event => { if (busy || event.button) return; drag.current = { id: item.id, x: event.clientX, y: event.clientY, after: false, moved: false }; event.currentTarget.setPointerCapture(event.pointerId); },
          onPointerMove: event => {
            const current = drag.current; if (!current) return;
            if (Math.hypot(current.x - event.clientX, current.y - event.clientY) > 5) current.moved = true;
            if (!current.moved) return;
            const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-inbox-id]');
            const rect = target?.getBoundingClientRect();
            current.over = target?.dataset.inboxId; current.after = !!rect && event.clientY > rect.top + rect.height / 2;
            setDragging({ ...current });
            const scroll = event.currentTarget.closest('.inbox-scroll');
            const bounds = scroll?.getBoundingClientRect();
            if (bounds && scroll) { if (event.clientY < bounds.top + 36) scroll.scrollTop -= 18; else if (event.clientY > bounds.bottom - 36) scroll.scrollTop += 18; }
          },
          onPointerUp: event => {
            const current = drag.current; drag.current = undefined; setDragging(undefined);
            if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
            if (!current?.moved || !current.over || current.over === current.id) return;
            const order = data!.items.filter(item => item.id !== current.id);
            const target = order.findIndex(item => item.id === current.over);
            if (target >= 0) void move(current.id, current.after ? order[target + 1]?.id ?? null : current.over);
          },
          onPointerCancel: () => { drag.current = undefined; setDragging(undefined); },
          onKeyDown: event => {
            if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
            event.preventDefault();
            if (event.key === 'ArrowUp' && index > 0) void move(item.id, items[index - 1].id);
            else if (event.key === 'ArrowDown' && index < items.length - 1) {
              const order = data!.items.filter(other => other.id !== item.id);
              const next = order.findIndex(other => other.id === items[index + 1].id);
              void move(item.id, order[next + 1]?.id ?? null);
            }
          },
        }} />)}</div>
      {draft && <div ref={editor} className="inbox-new-row" data-inbox-editor="new">
        {capture.editing ? <InlineEditor value={draft.markdown} label="新需求 Markdown" onChange={markdown => capture.update({ markdown })}
          onFinish={() => { void capture.finish(); }} onComposing={capture.setComposing} status={capture.status} />
          : <><div className="inbox-edit-target inbox-content" tabIndex={0} role="group" aria-label="需求内容，双击或按 Enter 编辑"
            onDoubleClick={event => { if (!(event.target as Element).closest('a, button, input, select, textarea, summary')) capture.begin(); }} onKeyDown={event => { if (event.target === event.currentTarget && ['Enter', 'F2'].includes(event.key)) { event.preventDefault(); capture.begin(); } }}>
            <MarkdownContent text={draft.markdown} readOnly className="inbox-markdown" /></div><span className="inbox-save-status" role="status">{capture.status}</span></>}
        {capture.conflict && <p className="inbox-conflict">这条需求已有更新。<button disabled={capture.saving} onClick={() => capture.resolve(true)}>{capture.removing ? '按最新版本移除' : '保留草稿，以最新版本保存'}</button><button disabled={capture.saving} onClick={() => capture.resolve(false)}>载入最新内容</button></p>}
        {capture.error && <p className="inbox-error" role="alert">{capture.error}<button onClick={() => { void capture.retry(); }}>重试</button></p>}
      </div>}
      {actionError && <p className="inbox-error" role="alert">{actionError}</p>}
    </div>
    <footer className="inbox-add-bar"><button onClick={() => { void add(); }} disabled={!data}><Plus size={16} />添加需求</button></footer>
  </section>;
}
