import { t } from '../../lib/i18n';
import type { ReactNode } from 'react';
import { ArrowUp, Check, ChevronDown, FileCode2, GitBranch, Pause, Play, SquareTerminal } from 'lucide-react';
import { Avatar, Status } from './ui';
import type { Agent, Project, Task } from './model';

export function TaskDetail({ task, agent, project, onUpdate, onShowFiles, collaboration, draft, onDraftChange }: {
  task: Task; agent?: Agent; project?: Project; onUpdate: (task: Task) => void; onShowFiles: () => void;
  collaboration: ReactNode; draft: string; onDraftChange: (text: string) => void;
}) {
  const steps = [t('org_preview.plan_understand'), project?.repository ? t('org_preview.plan_implement') : t('org_preview.plan_research'), t('org_preview.plan_validate')];
  return <div className="ow-task-detail">
    <div className="ow-task-scroll"><div className="ow-task-content">
      <div className="ow-task-eyebrow"><span>{task.id}</span><Status status={task.status} /><span className="ow-small-label">{t('org_preview.demo_run')}</span></div>
      <h1>{task.title}</h1>
      <div className="ow-task-meta"><span><Avatar agent={agent} small />{agent?.name}<span className="ow-muted">{agent?.role}</span></span><span><GitBranch size={14} />{task.worktree ? `task/${task.id.toLowerCase()}` : project?.repository ? 'main' : t('org_preview.standalone_task_directory')}</span></div>
      <div className="ow-primary-collaboration">{collaboration}</div>
      <article className="ow-message ow-user-message"><div className="ow-message-heading"><span className="ow-user-avatar">Y</span><strong>{t('org_preview.you')}</strong><time>{task.time}</time></div><p>{task.prompt}</p></article>
      <article className="ow-message"><div className="ow-message-heading"><Avatar agent={agent} small /><strong>{agent?.name}</strong><span className="ow-role-label">{agent?.role}</span></div>
        <p>{project ? t('org_preview.acknowledge_project', { project: project.name }) : t('org_preview.acknowledge_task')}</p>
        <div className="ow-plan"><div className="ow-plan-title"><span>{t('org_preview.execution_plan')}</span><span>{Math.min(task.progress, 3)} / 3</span></div>{steps.map((step, index) => <div className={index < task.progress ? 'ow-step ow-step-done' : 'ow-step'} key={step}><span>{index < task.progress ? <Check size={12} /> : index + 1}</span>{step}{index === task.progress && task.status === 'running' && <span className="ow-step-current">{t('org_preview.in_progress')}</span>}</div>)}</div>
        <details className="ow-execution"><summary><SquareTerminal size={15} /><span>{t('org_preview.execution_log')}</span><span className="ow-muted">{t('org_preview.step_count', { count: task.progress + 1 })}</span><ChevronDown size={13} /></summary><div><code>$ cd {task.cwd}</code><code>{t('org_preview.directory_ready')}</code><code>{t('org_preview.connected_via', { connector: agent?.connector ?? 'ACP', agent: agent?.name ?? 'Agent' })}</code>{task.progress > 0 && <code>{t('org_preview.context_read')}</code>}{task.progress > 1 && <code>{t('org_preview.artifacts_generated')}</code>}{task.progress > 2 && <code>{t('org_preview.demo_finished')}</code>}</div></details>
        {task.progress > 1 && <><p>{project?.repository ? t('org_preview.changes_ready') : t('org_preview.documents_ready')}</p><button className="ow-artifact" onClick={onShowFiles}><FileCode2 size={19} /><span><strong>{project?.repository ? t('org_preview.view_task_changes') : t('org_preview.view_task_documents')}</strong><small>{project?.name ?? t('org_preview.standalone_task')} · {project?.repository ? t('org_preview.file_count', { count: 3 }) : t('org_preview.task_artifacts')}</small></span><span>→</span></button></>}
      </article>
      {task.messages.map((text, index) => <article className="ow-message ow-user-message" key={`${index}-${text}`}><div className="ow-message-heading"><span className="ow-user-avatar">Y</span><strong>{t('org_preview.you')}</strong></div><p>{text}</p><small className="ow-muted">{t('org_preview.followup_added')}</small></article>)}
      <div className="ow-run-state" role="status"><Status status={task.status} /><span>{task.status === 'running' ? t('org_preview.agent_working', { value0: agent?.name ?? 'Agent' }) : task.status === 'paused' ? t('org_preview.paused_task_help') : t('org_preview.finished_task_help')}</span>{(task.status === 'running' || task.status === 'paused') && <button onClick={() => onUpdate({ ...task, status: task.status === 'running' ? 'paused' : 'running' })}>{task.status === 'running' ? <><Pause size={13} />{t('org_preview.pause')}</> : <><Play size={13} />{t('org_preview.resume')}</>}</button>}</div>
    </div></div>
    <div className="ow-followup-wrap"><form className="ow-followup" onSubmit={event => { event.preventDefault(); if (!draft.trim()) return; onUpdate({ ...task, messages: [...task.messages, draft.trim()], status: 'running', progress: 1 }); }}><label className="ow-sr-only" htmlFor="ow-followup">{t('org_preview.additional_requirements')}</label><input id="ow-followup" placeholder={t('org_preview.followup_placeholder')} value={draft} onChange={event => onDraftChange(event.target.value)} /><button className="ow-send" type="submit" disabled={!draft.trim()} aria-label={t('org_preview.send_requirements')}><ArrowUp size={17} /></button></form><div className="ow-followup-caption"><span><Avatar agent={agent} small />{agent?.name} · {agent?.connector}</span><span>{t('org_preview.main_session_summary')}</span></div></div>
  </div>;
}
