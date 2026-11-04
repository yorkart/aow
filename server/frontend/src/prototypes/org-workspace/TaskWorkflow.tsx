import { t } from '../../lib/i18n';
import { ArrowRight, Check, ChevronRight, CircleCheck, Eye, GitFork, ShieldCheck, Workflow } from 'lucide-react';
import { SessionTree } from './SessionViews';
import { Avatar } from './ui';
import type { Agent, Task } from './model';

export function TaskWorkflow({ task, tasks, agents, onOpen, onSpawn, onProperties, onApprove }: {
  task: Task; tasks: Task[]; agents: Agent[]; onOpen: (id: string) => void; onSpawn: (parentId: string) => void; onProperties: () => void; onApprove: () => void;
}) {
  const workspace = task.workspace!;
  const properties = workspace.properties;
  const stages = [
    { id: 'execute', title: t('org_preview.execute'), subtitle: t('org_preview.execute_description'), agentId: task.agentId, icon: GitFork },
    ...(properties.reviewer ? [{ id: 'review', title: t('org_preview.review'), subtitle: t('org_preview.review_description'), agentId: properties.reviewer, icon: Eye }] : []),
    ...(properties.approver ? [{ id: 'approval', title: t('org_preview.approve'), subtitle: t('org_preview.approve_description'), agentId: properties.approver, icon: ShieldCheck }] : []),
    { id: 'done', title: t('org_preview.done'), subtitle: t('org_preview.done_description'), agentId: '', icon: CircleCheck },
  ];
  const currentIndex = stages.findIndex(item => item.id === workspace.phase);
  const blockedBy = tasks.find(item => item.id === properties.blockedBy && item.status !== 'done');
  const dependencies = workspace.sessions.filter(item => item.dependsOn.length > 0);
  return <div className="ow-workflow-page">
    <header className="ow-page-heading"><div><span className="ow-eyebrow">{task.id} / {t('org_preview.execution')}</span><h1><Workflow size={23} />{t('org_preview.workflow')}</h1><p>{t('org_preview.workflow_description')}</p></div><button className="ow-secondary" onClick={onProperties}>{t('org_preview.configure_execution_roles')}<ChevronRight size={13} /></button></header>
    {(task.status === 'paused' || blockedBy) && <div className="ow-workflow-notice">{blockedBy ? t('org_preview.blocked_task_notice', { value0: blockedBy.id }) : t('org_preview.paused_workflow_notice')}</div>}
    <div className="ow-flow-stages">{stages.map((stage, index) => <div key={stage.id} className="ow-flow-stage-wrap"><button className={`ow-flow-stage ${stage.id === workspace.phase ? 'active' : ''} ${index < currentIndex ? 'complete' : ''}`} onClick={() => {
      const session = workspace.sessions.find(item => stage.id === 'execute' ? item.id === 'main' : item.stage === stage.id);
      if (session) onOpen(session.id); else if (stage.id === 'done') onOpen('main'); else onProperties();
    }}><span className="ow-stage-number">{index < currentIndex ? <Check size={13} /> : <stage.icon size={16} />}</span><strong>{stage.title}<small>{stage.id === workspace.phase ? task.status === 'paused' ? t('org_preview.paused') : stage.id === 'approval' ? t('org_preview.awaiting_approval') : stage.id === 'done' ? t('org_preview.completed') : t('org_preview.in_progress') : index < currentIndex ? t('org_preview.completed') : t('org_preview.not_started')}</small></strong><p>{stage.subtitle}</p>{stage.agentId && <span className="ow-stage-agent"><Avatar agent={agents.find(item => item.id === stage.agentId)} small />{agents.find(item => item.id === stage.agentId)?.name}</span>}</button>{index < stages.length - 1 && <ArrowRight size={14} />}</div>)}</div>
    {workspace.phase === 'approval' && <div className="ow-approval-banner"><ShieldCheck size={21} /><div><strong>{t('org_preview.execution_review_complete')}</strong><p>{t('org_preview.approval_assignee', { name: agents.find(item => item.id === properties.approver)?.name ?? 'Agent' })}</p></div><button className="ow-primary" disabled={task.status === 'paused' || Boolean(blockedBy)} onClick={onApprove}>{t('org_preview.simulate_approval')}<Check size={14} /></button></div>}
    <div className="ow-workflow-columns"><section><h2>{t('org_preview.session_hierarchy')} <span>{t('org_preview.delegation')}</span></h2><SessionTree workspace={workspace} agents={agents} onOpen={onOpen} onSpawn={onSpawn} /></section><section><h2>{t('org_preview.execution_dependencies')} <span>{t('org_preview.dependencies')}</span></h2><div className="ow-dependency-list">{dependencies.map(session => <div key={session.id}><button onClick={() => onOpen(session.id)}>{session.id === 'main' ? t('org_preview.main_session_results') : session.title}<ChevronRight size={12} /></button><small>{t('org_preview.waiting')}</small>{session.dependsOn.map(id => { const dependency = workspace.sessions.find(item => item.id === id); return <button className="ow-dependency-chip" key={id} onClick={() => onOpen(id)}><span className={`ow-session-dot ow-session-${dependency?.status ?? 'waiting'}`} />{dependency?.title}</button>; })}</div>)}{!dependencies.length && <p className="ow-help">{t('org_preview.no_dependencies')}</p>}</div><p className="ow-property-note">{t('org_preview.session_relationships_help')}</p></section></div>
    <div className="ow-workflow-observers">{([{ key: 'monitor', title: t('org_preview.monitor'), icon: Eye }, { key: 'watchdog', title: t('org_preview.watchdog'), icon: ShieldCheck }] as const).map(role => <div key={role.key}><role.icon size={17} /><span>{role.title}</span><strong>{agents.find(item => item.id === properties[role.key])?.name ?? t('org_preview.not_configured')}</strong><small>{properties[role.key] ? t('org_preview.observer_demo') : t('org_preview.optional')}</small></div>)}</div>
    <p className="ow-demo-note">{t('org_preview.workflow_demo_help')}</p>
  </div>;
}
