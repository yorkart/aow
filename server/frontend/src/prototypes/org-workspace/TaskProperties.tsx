import { t, getLanguage } from '../../lib/i18n';
import { ArrowUpRight, Circle, Folder, GitBranch, LockKeyhole, SlidersHorizontal, Workflow } from 'lucide-react';
import { Avatar, Status } from './ui';
import { relationshipCandidates, type TaskProperties as Properties } from './workspaceModel';
import type { Agent, Project, Task } from './model';

export function TaskProperties({ task, project, agents, tasks, onChange, onTaskStatus, onOpenTask, onWorkflow }: {
  task: Task; project?: Project; agents: Agent[]; tasks: Task[]; onChange: (properties: Properties) => void;
  onTaskStatus: (status: Task['status']) => void; onOpenTask: (id: string) => void; onWorkflow: () => void;
}) {
  const workspace = task.workspace!;
  const properties = workspace.properties;
  const agent = agents.find(item => item.id === task.agentId);
  const subtasks = tasks.filter(item => item.workspace?.properties.parentId === task.id);
  const blocking = tasks.filter(item => item.workspace?.properties.blockedBy === task.id);
  const change = <K extends keyof Properties>(key: K, value: Properties[K]) => onChange({ ...properties, [key]: value, updated: new Date().toISOString() });
  const roles = [{ key: 'reviewer', label: t('org_preview.reviewers'), description: t('org_preview.reviewer_description') }, { key: 'approver', label: t('org_preview.approvers'), description: t('org_preview.approver_description') }, { key: 'monitor', label: t('org_preview.monitor'), description: t('org_preview.monitor_description') }, { key: 'watchdog', label: t('org_preview.watchdog'), description: t('org_preview.watchdog_description') }] as const;
  const date = (value: string) => new Date(value).toLocaleString(getLanguage(), { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  return <div className="ow-properties-page">
    <header className="ow-page-heading"><div><span className="ow-eyebrow">{task.id} / {t('org_preview.task')}</span><h1><SlidersHorizontal size={23} />{t('org_preview.properties')}</h1><p>{task.title}</p></div><button className="ow-secondary" onClick={onWorkflow}><Workflow size={14} />{t('org_preview.view_workflow')}</button></header>
    <div className="ow-properties-grid"><div>
      <section className="ow-property-section"><h2>{t('org_preview.work')}</h2>
        <div className="ow-property-row"><span>{t('org_preview.status')}</span><div><Status status={task.status} />{workspace.phase !== 'done' && <button className="ow-property-action" onClick={() => onTaskStatus(task.status === 'paused' ? workspace.phase === 'execute' ? 'running' : 'review' : 'paused')}>{task.status === 'paused' ? t('org_preview.resume_execution') : t('org_preview.pause_task')}</button>}</div></div>
        <div className="ow-property-row"><span>{t('org_preview.assignee')}</span><div><Avatar agent={agent} small /><span>{agent?.name}</span><small>{agent?.role}</small></div></div>
        <div className="ow-property-row"><span>{t('org_preview.project')}</span><div><Folder size={16} /><span>{project?.name ?? t('org_preview.no_project')}</span><LockKeyhole size={11} /></div></div>
        <label className="ow-property-row"><span>{t('org_preview.labels')}</span><input aria-label={t('org_preview.task_labels')} placeholder={t('org_preview.labels_placeholder')} value={properties.labels} onChange={event => change('labels', event.target.value)} /></label>
        <p className="ow-property-note">{t('org_preview.execution_context_help')}</p>
      </section>
      <section className="ow-property-section"><h2>{t('org_preview.relationships')}</h2>
        <label className="ow-property-row"><span>{t('org_preview.parent')}</span><select aria-label={t('org_preview.parent_task')} value={properties.parentId} onChange={event => change('parentId', event.target.value)}><option value="">{t('org_preview.none')}</option>{relationshipCandidates(task, tasks, 'parentId').map(item => <option key={item.id} value={item.id}>{item.id} · {item.title}</option>)}</select></label>
        <label className="ow-property-row"><span>{t('org_preview.blocked_by')}</span><select aria-label={t('org_preview.blocking_task')} value={properties.blockedBy} onChange={event => change('blockedBy', event.target.value)}><option value="">{t('org_preview.none')}</option>{relationshipCandidates(task, tasks, 'blockedBy').map(item => <option key={item.id} value={item.id}>{item.id} · {item.title}</option>)}</select></label>
        <div className="ow-property-row"><span>{t('org_preview.blocking')}</span><div className="ow-property-links">{blocking.length ? blocking.map(item => <button key={item.id} onClick={() => onOpenTask(item.id)}>{item.id}<ArrowUpRight size={12} /></button>) : <span className="ow-muted">{t('org_preview.none')}</span>}</div></div>
        <div className="ow-property-row"><span>{t('org_preview.subtasks')}</span><div className="ow-property-links">{subtasks.length ? subtasks.map(item => <button key={item.id} onClick={() => onOpenTask(item.id)}>{item.id}<ArrowUpRight size={12} /></button>) : <span className="ow-muted">{t('org_preview.none')}</span>}</div></div>
        <p className="ow-property-note">{t('org_preview.task_relationships_help')}</p>
      </section>
    </div><div>
      <section className="ow-property-section"><h2>{t('org_preview.execution')}</h2>{roles.map(role => <label className="ow-property-row ow-property-role" key={role.key}><span>{role.label}<small>{role.description}</small></span><select aria-label={role.label} disabled={workspace.phase !== 'execute'} value={properties[role.key]} onChange={event => change(role.key, event.target.value)}><option value="">{t('org_preview.none')}</option>{agents.map(item => <option key={item.id} value={item.id}>{item.name} · {item.role}</option>)}</select></label>)}<p className="ow-property-note">{t('org_preview.execution_roles_help')}</p></section>
      <section className="ow-property-section"><h2>{t('org_preview.about')}</h2><div className="ow-property-row"><span>{t('org_preview.origin')}</span><div><Circle size={15} />{properties.origin}</div></div><div className="ow-property-row"><span>{t('org_preview.created')}</span><time dateTime={properties.created}>{date(properties.created)}</time></div><div className="ow-property-row"><span>{t('org_preview.updated')}</span><time dateTime={properties.updated}>{date(properties.updated)}</time></div></section>
    </div></div>
    <div className="ow-properties-cwd"><GitBranch size={15} /><span>{task.worktree ? t('org_preview.separate_worktree') : project?.repository ? t('org_preview.project_repository') : t('org_preview.separate_directory')}</span><code>{task.cwd}</code></div>
  </div>;
}
