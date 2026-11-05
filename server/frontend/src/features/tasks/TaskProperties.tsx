import { useState, type ReactNode } from 'react';
import { ArrowUpRight, Folder, GitBranch, MessageSquare } from 'lucide-react';
import type { AowProject, AowWorktree } from '../../aow/types';
import { getLanguage, t } from '../../lib/i18n';
import { tasksApi, taskTitle, type WorkTask, type TaskStatus } from './api';
import { useTaskScope } from './TaskScope';
import { TaskStatusIcon, TaskStatusPicker } from './TaskStatus';
import './tasks.css';

function Property({ label, children }: { label: string; children: ReactNode }) {
  return <div className="task-property-row"><dt>{label}</dt><dd>{children}</dd></div>;
}

function Timestamp({ value }: { value: number | null }) {
  if (value == null) return <span className="task-property-empty">—</span>;
  const date = new Date(value);
  return <time dateTime={date.toISOString()} title={date.toLocaleString(getLanguage())}>{date.toLocaleString(getLanguage(), {
    year: 'numeric', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit',
  })}</time>;
}

export function TaskProperties({ project, worktree, conversations }: { project: AowProject; worktree: AowWorktree; conversations: ReactNode }) {
  const scope = useTaskScope();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  if (!scope || scope.task.is_router) return null;
  const { task } = scope;
  const related = scope.tasks.filter(item => item.project_id === task.project_id);
  const parent = related.find(item => item.id === task.source_task_id);
  const subtasks = related.filter(item => item.source_task_id === task.id);
  const changeStatus = async (status: TaskStatus) => {
    setSaving(true); setError('');
    try { await tasksApi.status(task.id, status); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSaving(false); }
  };
  const link = (item: WorkTask) => <button key={item.id} type="button" className="task-property-task" title={taskTitle(item)}
    disabled={!scope.openTask || item.archived} onClick={() => scope.openTask?.(item)}>
    {item.is_router ? <MessageSquare size={15} /> : <TaskStatusIcon status={item.status} size={15} />}<span>{taskTitle(item)}</span>
  </button>;
  const none = <span className="task-property-empty">{t('tasks.none')}</span>;
  const branch = worktree.detached ? `${t('tasks.detached_head')} · ${worktree.head.slice(0, 7)}` : worktree.branch;
  return <section className="task-properties" aria-label={t('tasks.properties')} aria-busy={saving}>
    <section className="task-property-group"><h3>{t('tasks.work')}</h3><dl>
      <Property label={t('tasks.status')}><TaskStatusPicker status={task.status} disabled={saving || task.archived} onChange={status => void changeStatus(status)} /></Property>
      <Property label={t('tasks.project')}><button type="button" className="task-property-link" title={project.name} disabled={!scope.openProject} onClick={scope.openProject}>
        <Folder size={16} /><span>{project.name}</span><ArrowUpRight size={13} />
      </button></Property>
    </dl>{error && <p className="task-properties-error" role="alert">{error}</p>}</section>
    <section className="task-property-group"><h3>{t('tasks.relationships')}</h3><dl>
      <Property label={t('tasks.parent')}>{parent ? link(parent) : task.source_task_id ? <span className="task-property-empty" title={task.source_task_id}>{task.source_task_id}</span> : none}</Property>
      <Property label={t('tasks.subtasks')}><div className="task-property-links">{subtasks.length ? subtasks.map(link) : none}</div></Property>
    </dl></section>
    <section className="task-property-group"><h3>{t('tasks.workspace')}</h3><dl>
      <Property label={t('tasks.branch')}><span className="task-property-path" title={branch || undefined}><GitBranch size={16} /><span>{branch || '—'}</span></span></Property>
      <Property label={t('tasks.folder')}><span className="task-property-path" title={task.workspace}><Folder size={16} /><span>{task.workspace}</span></span></Property>
    </dl></section>
    <section className="task-property-group"><h3>{t('tasks.about')}</h3><dl>
      <Property label={t('tasks.created')}><Timestamp value={task.created_at} /></Property>
      <Property label={t('tasks.updated')}><Timestamp value={task.updated_at} /></Property>
      <Property label={t('tasks.completed')}><Timestamp value={task.completed_at} /></Property>
    </dl></section>
    <section className="task-property-group"><h3>{t('navigation.conversation')}</h3>{conversations}</section>
  </section>;
}
