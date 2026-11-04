import { t } from '../../lib/i18n';
import { useEffect, useRef, type KeyboardEvent } from 'react';
import { FileText, GitFork, LockKeyhole, MessageSquare, Plus, SlidersHorizontal, SquareTerminal, Workflow, X } from 'lucide-react';
import type { TaskWorkspaceState, WorkspaceTab } from './workspaceModel';

export function WorkspaceTabs({ workspace, onSelect, onClose, onAdd }: {
  workspace: TaskWorkspaceState; onSelect: (id: string) => void; onClose: (id: string) => void; onAdd: () => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const reveal = () => list.current?.querySelector('[aria-selected="true"]')?.closest('.ow-workspace-tab')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    reveal();
    const observer = new ResizeObserver(reveal);
    if (list.current) observer.observe(list.current);
    return () => observer.disconnect();
  }, [workspace.activeTab]);
  const icon = (tab: WorkspaceTab) => {
    if (tab.kind === 'main') return <MessageSquare size={15} />;
    if (tab.kind === 'properties') return <SlidersHorizontal size={15} />;
    if (tab.kind === 'workflow') return <Workflow size={15} />;
    if (tab.kind === 'file') return <FileText size={15} />;
    return workspace.sessions.find(session => session.id === tab.target)?.kind === 'terminal' ? <SquareTerminal size={15} /> : <GitFork size={15} />;
  };
  const keyboard = (event: KeyboardEvent, index: number) => {
    const length = workspace.tabs.length;
    const next = event.key === 'ArrowRight' ? (index + 1) % length : event.key === 'ArrowLeft' ? (index + length - 1) % length : event.key === 'Home' ? 0 : event.key === 'End' ? length - 1 : -1;
    if (next >= 0) { event.preventDefault(); onSelect(workspace.tabs[next].id); list.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[next]?.focus(); }
    if (event.key === 'Delete' && index > 0) {
      event.preventDefault(); onClose(workspace.tabs[index].id);
      list.current?.querySelectorAll<HTMLButtonElement>('[role="tab"]')[index - 1]?.focus();
    }
  };
  return <div className="ow-tabs-shell">
    <div ref={list} role="tablist" aria-label={t('org_preview.workspace_tabs')} className="ow-workspace-tabs">{workspace.tabs.map((tab, index) => {
      const document = tab.kind === 'file' ? workspace.documents.find(item => item.path === tab.target) : undefined;
      const dirty = document && document.content !== document.saved;
      const session = workspace.sessions.find(item => item.id === tab.target);
      return <div role="presentation" key={tab.id} className={`ow-workspace-tab ${workspace.activeTab === tab.id ? 'active' : ''}`}>
        <button role="tab" id={`ow-tab-${index}`} aria-controls="ow-tab-content" aria-selected={workspace.activeTab === tab.id} tabIndex={workspace.activeTab === tab.id ? 0 : -1}
          onClick={() => onSelect(tab.id)} onKeyDown={event => keyboard(event, index)} title={tab.kind === 'main' ? t('org_preview.main_session_pinned') : tab.title}>
          {icon(tab)}<span>{tab.title}</span>{dirty && <span className="ow-unsaved" aria-label={t('org_preview.unsaved')} />}
          {tab.kind === 'main' ? <LockKeyhole className="ow-tab-lock" size={11} /> : session?.status === 'running' && <span className="ow-live-dot" />}
        </button>
        {tab.kind !== 'main' && <button className="ow-close-tab" aria-label={t('org_preview.close_tab', { value0: tab.title })} title={t('org_preview.close_tab_help')} onClick={() => onClose(tab.id)}><X size={13} /></button>}
      </div>;
    })}</div>
    <button className="ow-add-tab" aria-label={t('org_preview.add_workspace_tab')} title={t('org_preview.add_workspace_tab_help')} onClick={onAdd}><Plus size={18} /></button>
  </div>;
}
