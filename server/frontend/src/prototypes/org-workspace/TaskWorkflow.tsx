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
    { id: 'execute', title: '执行', subtitle: '主会话与子会话协作', agentId: task.agentId, icon: GitFork },
    ...(properties.reviewer ? [{ id: 'review', title: '审查', subtitle: '检查产物与执行结果', agentId: properties.reviewer, icon: Eye }] : []),
    ...(properties.approver ? [{ id: 'approval', title: '批准', subtitle: '等待批准决定', agentId: properties.approver, icon: ShieldCheck }] : []),
    { id: 'done', title: '完成', subtitle: '结果归集到任务', agentId: '', icon: CircleCheck },
  ];
  const currentIndex = stages.findIndex(item => item.id === workspace.phase);
  const blockedBy = tasks.find(item => item.id === properties.blockedBy && item.status !== 'done');
  const dependencies = workspace.sessions.filter(item => item.dependsOn.length > 0);
  return <div className="ow-workflow-page">
    <header className="ow-page-heading"><div><span className="ow-eyebrow">{task.id} / EXECUTION</span><h1><Workflow size={23} />Workflow</h1><p>一个任务，多段协作，统一汇总结果。</p></div><button className="ow-secondary" onClick={onProperties}>配置执行角色<ChevronRight size={13} /></button></header>
    {(task.status === 'paused' || blockedBy) && <div className="ow-workflow-notice">{blockedBy ? `任务被 ${blockedBy.id} 阻塞，等待它完成后继续。` : '任务已暂停，所有模拟执行暂时停止。'}</div>}
    <div className="ow-flow-stages">{stages.map((stage, index) => <div key={stage.id} className="ow-flow-stage-wrap"><button className={`ow-flow-stage ${stage.id === workspace.phase ? 'active' : ''} ${index < currentIndex ? 'complete' : ''}`} onClick={() => {
      const session = workspace.sessions.find(item => stage.id === 'execute' ? item.id === 'main' : item.stage === stage.id);
      if (session) onOpen(session.id); else if (stage.id === 'done') onOpen('main'); else onProperties();
    }}><span className="ow-stage-number">{index < currentIndex ? <Check size={13} /> : <stage.icon size={16} />}</span><strong>{stage.title}<small>{stage.id === workspace.phase ? task.status === 'paused' ? '已暂停' : stage.id === 'approval' ? '等待批准' : stage.id === 'done' ? '已完成' : '进行中' : index < currentIndex ? '已完成' : '待开始'}</small></strong><p>{stage.subtitle}</p>{stage.agentId && <span className="ow-stage-agent"><Avatar agent={agents.find(item => item.id === stage.agentId)} small />{agents.find(item => item.id === stage.agentId)?.name}</span>}</button>{index < stages.length - 1 && <ArrowRight size={14} />}</div>)}</div>
    {workspace.phase === 'approval' && <div className="ow-approval-banner"><ShieldCheck size={21} /><div><strong>执行与审查已完成</strong><p>由 {agents.find(item => item.id === properties.approver)?.name} 承担批准角色，等待本次决定。</p></div><button className="ow-primary" disabled={task.status === 'paused' || Boolean(blockedBy)} onClick={onApprove}>模拟批准<Check size={14} /></button></div>}
    <div className="ow-workflow-columns"><section><h2>会话层级 <span>Delegation</span></h2><SessionTree workspace={workspace} agents={agents} onOpen={onOpen} onSpawn={onSpawn} /></section><section><h2>执行依赖 <span>Dependencies</span></h2><div className="ow-dependency-list">{dependencies.map(session => <div key={session.id}><button onClick={() => onOpen(session.id)}>{session.id === 'main' ? '主会话汇总' : session.title}<ChevronRight size={12} /></button><small>等待</small>{session.dependsOn.map(id => { const dependency = workspace.sessions.find(item => item.id === id); return <button className="ow-dependency-chip" key={id} onClick={() => onOpen(id)}><span className={`ow-session-dot ow-session-${dependency?.status ?? 'waiting'}`} />{dependency?.title}</button>; })}</div>)}{!dependencies.length && <p className="ow-help">当前没有会话依赖。可从主会话派生子会话，或独立创建辅助会话。</p>}</div><p className="ow-property-note">父子关系表示委派来源，依赖关系决定何时可以执行。</p></section></div>
    <div className="ow-workflow-observers">{([{ key: 'monitor', title: 'Monitor', icon: Eye }, { key: 'watchdog', title: 'Watchdog', icon: ShieldCheck }] as const).map(role => <div key={role.key}><role.icon size={17} /><span>{role.title}</span><strong>{agents.find(item => item.id === properties[role.key])?.name ?? '未配置'}</strong><small>{properties[role.key] ? '观察角色 · 演示配置' : '可选'}</small></div>)}</div>
    <p className="ow-demo-note">演示流程按执行 → 审查 → 批准推进；只有配置了角色的节点才会出现。</p>
  </div>;
}
