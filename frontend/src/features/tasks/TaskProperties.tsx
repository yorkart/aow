import { useEffect, useState } from 'react';
import { ListTodo } from 'lucide-react';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { t } from '../../lib/i18n';
import { taskStatuses, tasksApi, taskTitle, type TaskStatus } from './api';
import { useTaskScope } from './TaskScope';
import './tasks.css';

export function TaskProperties() {
  const scope = useTaskScope();
  const task = scope?.task;
  const [prompt, setPrompt] = useState(task?.prompt ?? '');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => { setPrompt(task?.prompt ?? ''); setError(''); }, [task?.id, task?.prompt]);
  if (!scope || !task) return null;
  const parent = scope.tasks.find(item => item.id === task.source_task_id);
  const save = async (action: () => Promise<unknown>) => {
    setSaving(true); setError('');
    try { await action(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSaving(false); }
  };
  return <section className="task-properties" aria-label={t('tasks.properties')}>
    <AowPanelStack><AowPanel title={t('tasks.properties')} icon={<ListTodo />} collapsible={false}>
      <h3>{taskTitle(task)}</h3>
      {task.is_router ? <p>{t('tasks.router_hint')}</p> : <>
        <label>{t('tasks.business_status')}<select aria-label={t('tasks.business_status')} value={task.status ?? 'todo'} disabled={saving || task.archived}
          onChange={event => { const status = event.target.value as TaskStatus; void save(() => tasksApi.status(task.id, status)); }}>
          {taskStatuses.map(status => <option key={status} value={status}>{t('tasks.status_' + status)}</option>)}
        </select></label>
        <form onSubmit={event => { event.preventDefault(); void save(() => tasksApi.update(task.id, { prompt })); }}>
          <label>{t('tasks.prompt')}<textarea aria-label={t('tasks.prompt')} value={prompt} required maxLength={131072} disabled={saving || task.archived} onChange={event => setPrompt(event.target.value)} /></label>
          <button type="submit" disabled={saving || task.archived || !prompt.trim() || prompt === task.prompt}>{t(saving ? 'tasks.saving' : 'tasks.save')}</button>
        </form>
      </>}
      <dl><dt>{t('tasks.workspace')}</dt><dd>{task.workspace}</dd>
        {parent && <><dt>{t('tasks.parent')}</dt><dd><button type="button" disabled={!scope.openTask || parent.archived} onClick={() => scope.openTask?.(parent)}>{taskTitle(parent)}</button></dd></>}
      </dl>
      {error && <p role="alert">{error}</p>}
    </AowPanel></AowPanelStack>
  </section>;
}
