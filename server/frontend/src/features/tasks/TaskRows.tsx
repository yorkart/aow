import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Archive, ChevronRight, MessageSquare, Pencil, Pin, PinOff, RotateCcw, Trash2 } from 'lucide-react';
import { AowListRow } from '../../components/AowListRow';
import { t } from '../../lib/i18n';
import { taskTree, type TaskNode } from './tree';
import { taskTitle, type WorkTask } from './api';
import { TaskStatusIcon } from './TaskStatus';

export function TaskRows({ tasks, activeId, onSelect, onEdit, onPin, onArchive, onRemove }: {
  tasks: WorkTask[]; activeId?: string;
  onSelect: (task: WorkTask) => void; onEdit: (task: WorkTask) => void;
  onPin: (task: WorkTask) => void; onArchive: (task: WorkTask) => void; onRemove: (task: WorkTask) => void;
}) {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [menu, setMenu] = useState<{ task: WorkTask; x: number; y: number }>();
  const menuElement = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    const close = (event: PointerEvent) => { if (!menuElement.current?.contains(event.target as Node)) setMenu(undefined); };
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') setMenu(undefined); };
    window.addEventListener('pointerdown', close); window.addEventListener('keydown', key);
    menuElement.current?.querySelector('button')?.focus();
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('keydown', key); };
  }, [menu]);
  const action = (callback: (task: WorkTask) => void) => { if (menu) callback(menu.task); setMenu(undefined); };
  const render = ({ task, children }: TaskNode) => <div key={task.id} className="task-tree-node" data-task-id={task.id}>
    <div className="task-tree-row">
      {children.length > 0 && <button className="task-tree-toggle" type="button" aria-label={t('tasks.children', { task: taskTitle(task) })} aria-expanded={!collapsed.has(task.id)}
        onClick={() => setCollapsed(current => { const next = new Set(current); if (next.has(task.id)) next.delete(task.id); else next.add(task.id); return next; })}><ChevronRight className={!collapsed.has(task.id) ? 'expanded' : undefined} size={13} /></button>}
      <AowListRow className={`project-aow-task-row${activeId === task.id ? ' active' : ''}`} title={taskTitle(task)} tooltip={`${task.prompt}\n${task.workspace}`} active={activeId === task.id}
        icon={task.is_orchestrator ? <MessageSquare /> : <TaskStatusIcon status={task.status} />} actions={task.is_orchestrator ? <></> : undefined} openLabel={taskTitle(task)} onOpen={() => onSelect(task)} menuLabel={t('tasks.actions')}
        menuExpanded={menu?.task.id === task.id} onMenu={(x, y) => setMenu({ task, x, y })} />
    </div>
    {!collapsed.has(task.id) && children.length > 0 && <div className="task-tree-children">{children.map(render)}</div>}
  </div>;
  return <><div className="project-aow-task-list">{taskTree(tasks).map(render)}</div>{menu && createPortal(<div ref={menuElement} role="menu" aria-label={t('tasks.actions')} className="project-aow-context-menu"
    style={{ left: Math.max(8, Math.min(menu.x, window.innerWidth - 240)), top: Math.max(8, Math.min(menu.y, window.innerHeight - 190)) }}>
    <button role="menuitem" onClick={() => action(onEdit)}><Pencil />{t('tasks.edit')}</button>
    <button role="menuitem" onClick={() => action(onPin)}>{menu.task.pinned ? <PinOff /> : <Pin />}{t(menu.task.pinned ? 'tasks.unpin' : 'tasks.pin')}</button>
    <button role="menuitem" onClick={() => action(onArchive)}>{menu.task.archived ? <RotateCcw /> : <Archive />}{t(menu.task.archived ? 'tasks.restore' : 'tasks.archive')}</button>
    <button role="menuitem" onClick={() => action(onRemove)}><Trash2 />{t('tasks.delete')}</button>
  </div>, document.body)}</>;
}
