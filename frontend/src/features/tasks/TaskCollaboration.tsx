import { useState } from 'react';
import { t } from '../../lib/i18n';
import { taskTitle, tasksApi, type WorkTask, type TaskResult } from './api';
import { useTaskScope } from './TaskScope';
import './tasks.css';

function ResultCard({ task, onOpen }: { task: WorkTask; onOpen?: (task: WorkTask) => void }) {
  const [result, setResult] = useState<TaskResult>();
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const inspect = async () => {
    setLoading(true); setError('');
    try { setResult(await tasksApi.get(task.id)); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setLoading(false); }
  };
  return <div className="task-collaboration-card">
    <div><button type="button" className="task-collaboration-title" disabled={!onOpen || task.archived} onClick={() => onOpen?.(task)}>{taskTitle(task)}</button>
      <button type="button" disabled={loading} onClick={() => { void inspect(); }}>{t(loading ? 'conversations.loading' : 'tasks.inspect_result')}</button></div>
    {result && <div><small>{t('tasks.runtime_status')}: {result.runtime?.run_state ? t('inbox.' + result.runtime.run_state) : result.runtime?.status ?? t('tasks.runtime_unavailable')}{result.runtime?.pending_permissions ? ` · ${t('navigation.awaiting_confirmation')}` : ''}</small>
      {result.runtime?.error && <p role="alert">{result.runtime.error}</p>}
      {result.runtime?.response ? <pre>{result.runtime.response}</pre> : <p>{t('tasks.no_response')}</p>}</div>}
    {error && <p role="alert">{error}</p>}
  </div>;
}

export function TaskCollaboration() {
  const scope = useTaskScope();
  if (!scope) return null;
  const source = scope.tasks.find(task => task.id === scope.task.source_task_id);
  const children = scope.tasks.filter(task => !task.archived && task.id !== scope.task.id && (scope.task.is_router ? task.project_id === scope.task.project_id : task.source_task_id === scope.task.id));
  if (!source && !children.length) return null;
  return <div className="task-collaboration">
    {source && <p>{t('tasks.delegated_by')}: <button type="button" disabled={!scope.openTask || source.archived} onClick={() => scope.openTask?.(source)}>{taskTitle(source)}</button></p>}
    {!!children.length && <details open><summary>{t(scope.task.is_router ? 'tasks.project_progress' : 'tasks.delegated_tasks')} ({children.length})</summary>
      {children.map(task => <ResultCard key={task.id} task={task} onOpen={scope.openTask} />)}</details>}
  </div>;
}
