import { useState } from 'react';
import { useAowTabNavigation } from '../../aow/tabNavigation';
import { t } from '../../lib/i18n';
import { rolesApi, type AgentRole } from '../agent-roles/api';
import { taskStatuses, type TaskStatus, taskTitle, tasksApi, type WorkTask, type TaskResult } from './api';
import { useTaskScope } from './TaskScope';
import './tasks.css';

function ResultCard({ task, onOpen }: { task: WorkTask; onOpen?: (task: WorkTask) => void }) {
  const navigate = useAowTabNavigation();
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [result, setResult] = useState<TaskResult>();
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const inspect = async () => {
    setLoading(true); setError('');
    try { const [result, roles] = await Promise.all([tasksApi.get(task.id), rolesApi.list().catch(() => [])]); setResult(result); setRoles(roles); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setLoading(false); }
  };
  return <div className="task-collaboration-card">
    <div><button type="button" className="task-collaboration-title" disabled={!onOpen || task.archived} onClick={() => onOpen?.(task)}>{taskTitle(task)}</button>
      <button type="button" disabled={loading} onClick={() => { void inspect(); }}>{t(loading ? 'conversations.loading' : 'tasks.inspect_result')}</button></div>
    {result && <div><small>{t('tasks.business_status')}: {t('tasks.status_' + (task.status ?? result.task.status ?? 'todo'))} · </small><small>{t(result.runtime?.run_state ? 'inbox.execution' : 'tasks.runtime_status')}: {result.runtime?.run_state ? t('inbox.' + result.runtime.run_state) : result.runtime?.status ?? t('tasks.runtime_unavailable')}{result.runtime?.pending_permissions ? ` · ${t('navigation.awaiting_confirmation')}` : ''}</small>
      {result.runtime?.error && <p role="alert">{result.runtime.error}</p>}
      {result.runtime?.response ? <pre>{result.runtime.response}</pre> : <p>{t('tasks.no_response')}</p>}
      {!!result.runs?.length && <details><summary>{t('tasks.execution_history')} ({result.runs.length})</summary>
        {result.runs.map(run => <details key={run.id} className="task-collaboration-card"><summary>{t('inbox.' + run.state)} · {t('tasks.trigger_' + (run.trigger ?? 'message'))} · {roles.find(role => role.id === run.execution?.agent_role_id)?.name ?? run.execution?.agent_id ?? run.thread_id}</summary>
          <p>{t('tasks.run_id')}: {run.id}</p>
          {run.caused_by_run_id && <p>{t('tasks.triggered_by')}: {run.caused_by_run_id}</p>}
          <p><button type="button" onClick={() => { void navigate({ type: 'acp', thread_id: run.thread_id, workTaskId: task.id }).catch(reason => setError(String(reason))); }}>{t('tasks.conversation')}: {run.thread_id}</button></p>
          {run.execution && <p>{t('tasks.workspace')}: {run.execution.workspace}{run.execution.target_revision ? ` · ${run.execution.target_revision}` : ''}</p>}
          {run.error && <p role="alert">{run.error}</p>}{run.result && <pre>{run.result}</pre>}
        </details>)}
      </details>}
    </div>}
    {error && <p role="alert">{error}</p>}
  </div>;
}

export function TaskCollaboration() {
  const scope = useTaskScope();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  if (!scope) return null;
  const source = scope.tasks.find(task => task.id === scope.task.source_task_id);
  const children = scope.tasks.filter(task => !task.archived && task.id !== scope.task.id && (scope.task.is_router ? task.project_id === scope.task.project_id : task.source_task_id === scope.task.id));
  const changeStatus = async (status: TaskStatus) => {
    setSaving(true); setError('');
    try { await tasksApi.status(scope.task.id, status); }
    catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setSaving(false); }
  };
  return <div className="task-collaboration">
    {!scope.task.is_router && <label>{t('tasks.business_status')} <select aria-label={t('tasks.business_status')} value={scope.task.status ?? 'todo'} disabled={saving || scope.task.archived} onChange={event => { void changeStatus(event.target.value as TaskStatus); }}>
      {taskStatuses.map(status => <option key={status} value={status}>{t('tasks.status_' + status)}</option>)}
    </select></label>}
    {error && <p role="alert">{error}</p>}
    <ResultCard task={scope.task} />
    {source && <p>{t('tasks.delegated_by')}: <button type="button" disabled={!scope.openTask || source.archived} onClick={() => scope.openTask?.(source)}>{taskTitle(source)}</button></p>}
    {!!children.length && <details open><summary>{t(scope.task.is_router ? 'tasks.project_progress' : 'tasks.delegated_tasks')} ({children.length})</summary>
      {children.map(task => <ResultCard key={task.id} task={task} onOpen={scope.openTask} />)}</details>}
  </div>;
}
