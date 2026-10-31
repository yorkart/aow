import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Archive, ListTodo, Pencil, Pin, PinOff, RotateCcw, Trash2 } from 'lucide-react';
import { AowListRow } from '../../components/AowListRow';
import { t } from '../../lib/i18n';
import { taskTitle, type WorkTask } from './api';

export function TaskRows({ tasks, allTasks, activeId, onSelect, onEdit, onPin, onArchive, onRemove }: {
  tasks: WorkTask[]; allTasks: WorkTask[]; activeId?: string;
  onSelect: (task: WorkTask) => void; onEdit: (task: WorkTask) => void;
  onPin: (task: WorkTask) => void; onArchive: (task: WorkTask) => void; onRemove: (task: WorkTask) => void;
}) {
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
  return <><div className="project-aow-task-list">{tasks.map(task => {
    const dependencies = task.dependencies.map(id => allTasks.find(item => item.id === id)).filter((item): item is WorkTask => !!item);
    const detail = dependencies.length ? `${t('tasks.dependencies')}: ${dependencies.map(taskTitle).join(', ')}` : task.workspace;
    return <AowListRow key={task.id} className={`project-aow-task-row${activeId === task.id ? ' active' : ''}`} title={taskTitle(task)} tooltip={`${task.prompt}\n${detail}`} active={activeId === task.id}
      icon={<ListTodo />} openLabel={taskTitle(task)} onOpen={() => onSelect(task)} menuLabel={t('tasks.actions')}
      menuExpanded={menu?.task.id === task.id} onMenu={(x, y) => setMenu({ task, x, y })}><small title={detail}>{task.archived ? t('tasks.archived') : dependencies.length ? t('tasks.dependency_count', { count: dependencies.length }) : ''}</small></AowListRow>;
  })}</div>{menu && createPortal(<div ref={menuElement} role="menu" aria-label={t('tasks.actions')} className="project-aow-context-menu"
    style={{ left: Math.max(8, Math.min(menu.x, window.innerWidth - 240)), top: Math.max(8, Math.min(menu.y, window.innerHeight - 190)) }}>
    <button role="menuitem" onClick={() => action(onEdit)}><Pencil />{t('tasks.edit')}</button>
    <button role="menuitem" onClick={() => action(onPin)}>{menu.task.pinned ? <PinOff /> : <Pin />}{t(menu.task.pinned ? 'tasks.unpin' : 'tasks.pin')}</button>
    <button role="menuitem" onClick={() => action(onArchive)}>{menu.task.archived ? <RotateCcw /> : <Archive />}{t(menu.task.archived ? 'tasks.restore' : 'tasks.archive')}</button>
    <button role="menuitem" onClick={() => action(onRemove)}><Trash2 />{t('tasks.delete')}</button>
  </div>, document.body)}</>;
}
