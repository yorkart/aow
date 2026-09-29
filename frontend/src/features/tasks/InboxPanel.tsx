import { useCallback, useEffect, useState } from 'react';
import { Inbox, MoreHorizontal, Plus, ArrowUpRight, Check, FileText, Pencil, Trash2 } from 'lucide-react';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowIconButton } from '../../components/AowIconButton';
import { AowListRow } from '../../components/AowListRow';
import { AowListRowMeta, AowListRowStatus } from '../../components/AowListRowMeta';
import { tasksApi, taskError, useInboxPage, type useTaskBoard } from './api';
import { TaskDialog } from './TaskDialog';
import { InboxMenu } from './InboxMenu';
import { InboxEditor } from './InboxEditor';
import { IssuePanel } from './IssuePanel';
import { inboxAge } from './presentation';
import type { BoardTask, InboxItem, InboxSummary } from './types';

type InboxMenuState = { kind: 'options'; anchor: HTMLButtonElement; x: number; y: number }
  | { kind: 'item'; item: InboxSummary; x: number; y: number };

export function InboxPanel({ state, onConvert, onTask }: { state: ReturnType<typeof useTaskBoard>; onConvert: (item: InboxItem) => void; onTask: (task: BoardTask) => void }) {
  const [editor, setEditor] = useState<InboxItem | 'new'>();
  const [showConverted, setShowConverted] = useState(false);
  const inbox = useInboxPage(state.projectId, showConverted, state.visible);
  const [menu, setMenu] = useState<InboxMenuState>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<InboxSummary>();
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    const refresh = () => { if (!document.hidden) setNow(Date.now()); };
    const timer = window.setInterval(refresh, 60_000);
    document.addEventListener('visibilitychange', refresh);
    return () => { window.clearInterval(timer); document.removeEventListener('visibilitychange', refresh); };
  }, []);
  const closeMenu = useCallback(() => setMenu(undefined), []);
  const items = inbox.data?.items ?? [];
  const openItem = async (item: InboxSummary, convert: boolean) => {
    if (busy) return;
    setBusy(true); setError('');
    try {
      const detail = await tasksApi.getInbox(item.id);
      if (convert) onConvert(detail); else setEditor(detail);
    } catch (error) { setError(taskError(error)); }
    finally { setBusy(false); }
  };
  return <><AowPanelStack className="tasks-inbox"><AowPanel title="Inbox" icon={<Inbox />} bodyRole="list" details={<span>{inbox.data?.total ?? 0}</span>} onCollapsedChange={closeMenu} actions={<>
    <AowIconButton aria-label="录入需求" title="录入需求" onClick={() => setEditor('new')}><Plus /></AowIconButton>
    <AowIconButton aria-label="Inbox 显示选项" title="Inbox 显示选项" aria-haspopup="menu" aria-expanded={menu?.kind === 'options'} onClick={event => {
      const anchor = event.currentTarget;
      const bounds = anchor.getBoundingClientRect();
      setMenu(menu?.kind === 'options' ? undefined : { kind: 'options', anchor, x: bounds.right, y: bounds.bottom });
    }}><MoreHorizontal /></AowIconButton>
  </>}>
    {(inbox.error || error) && <p role="alert" className="tasks-error">{error || inbox.error}</p>}
    {!inbox.data ? <p className="tasks-empty">正在加载…</p> : !items.length ? <div className="tasks-empty"><Inbox size={26} /><p>想法先放在这里</p><button onClick={() => setEditor('new')}>录入需求</button></div> : items.map(item => <AowListRow className="" role="listitem" key={item.id}
      title={item.title} tooltip={item.title} icon={<FileText />} menuLabel="需求操作"
      onOpen={() => void openItem(item, false)} menuExpanded={menu?.kind === 'item' && menu.item.id === item.id}
      onMenu={(x, y) => setMenu({ kind: 'item', item, x, y })}>
      <AowListRowMeta status={<AowListRowStatus tone={item.task_ids.length ? 'success' : 'muted'}>{item.task_ids.length ? '已转换' : '未转换'}</AowListRowStatus>}>
        <time dateTime={item.created_at} title={`录入时间：${new Date(item.created_at).toLocaleString('zh-CN')}`}>
          {inboxAge(item.created_at, now)}
        </time>
      </AowListRowMeta>
    </AowListRow>)}
    {inbox.data?.next_cursor && <div className="tasks-empty"><button disabled={inbox.loading} onClick={inbox.loadMore}>{inbox.loading ? '正在加载…' : '加载更多需求'}</button></div>}
  </AowPanel><IssuePanel key={state.projectId} projectId={state.projectId} visible={state.visible} /></AowPanelStack>
    {menu?.kind === 'options' && <InboxMenu {...menu} label="Inbox 显示选项" onClose={closeMenu}>
      <button role="menuitemcheckbox" aria-checked={showConverted} onClick={() => { setShowConverted(!showConverted); closeMenu(); menu.anchor.focus(); }}>
        <Check style={{ visibility: showConverted ? 'visible' : 'hidden' }} />显示已转化
      </button>
    </InboxMenu>}
    {menu?.kind === 'item' && <InboxMenu {...menu} label={`${menu.item.title} 需求操作`} onClose={closeMenu}>
      <button role="menuitem" onClick={() => { void openItem(menu.item, true); closeMenu(); }}><ArrowUpRight />转为任务…</button>
      {menu.item.task_ids.map(id => { const task = state.board?.tasks.find(t => t.id === id); return task && <button role="menuitem" key={id} onClick={() => { onTask(task); closeMenu(); }}><ArrowUpRight />查看任务：{task.title}</button>; })}
      <button role="menuitem" onClick={() => { void openItem(menu.item, false); closeMenu(); }}><Pencil />编辑需求</button>
      {!menu.item.task_ids.length && <button role="menuitem" onClick={() => { setDeleting(menu.item); closeMenu(); }}><Trash2 />删除需求</button>}
    </InboxMenu>}
    {deleting && <TaskDialog title="删除需求" busy={busy} onClose={() => setDeleting(undefined)}><p>删除「{deleting.title}」？</p>{error && <p role="alert" className="tasks-error">{error}</p>}<footer><button disabled={busy} onClick={() => setDeleting(undefined)}>取消</button><button disabled={busy} onClick={() => { setBusy(true); void tasksApi.deleteInbox(deleting).then(() => setDeleting(undefined)).catch(e => setError(taskError(e))).finally(() => setBusy(false)); }}>删除</button></footer></TaskDialog>}
    {editor && <InboxEditor item={editor === 'new' ? undefined : editor} projectId={state.projectId} onClose={() => setEditor(undefined)} />}
  </>;
}
