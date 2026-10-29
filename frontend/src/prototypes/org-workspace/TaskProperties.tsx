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
  const roles = [{ key: 'reviewer', label: 'Reviewers', description: '审查执行结果' }, { key: 'approver', label: 'Approvers', description: '批准后完成任务' }, { key: 'monitor', label: 'Monitor', description: '观察执行状态' }, { key: 'watchdog', label: 'Watchdog', description: '发现停滞与异常' }] as const;
  const date = (value: string) => new Date(value).toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  return <div className="ow-properties-page">
    <header className="ow-page-heading"><div><span className="ow-eyebrow">{task.id} / TASK</span><h1><SlidersHorizontal size={23} />Properties</h1><p>{task.title}</p></div><button className="ow-secondary" onClick={onWorkflow}><Workflow size={14} />查看 Workflow</button></header>
    <div className="ow-properties-grid"><div>
      <section className="ow-property-section"><h2>WORK</h2>
        <div className="ow-property-row"><span>Status</span><div><Status status={task.status} />{workspace.phase !== 'done' && <button className="ow-property-action" onClick={() => onTaskStatus(task.status === 'paused' ? workspace.phase === 'execute' ? 'running' : 'review' : 'paused')}>{task.status === 'paused' ? '继续执行' : '暂停任务'}</button>}</div></div>
        <div className="ow-property-row"><span>Assignee</span><div><Avatar agent={agent} small /><span>{agent?.name}</span><small>{agent?.role}</small></div></div>
        <div className="ow-property-row"><span>Project</span><div><Folder size={16} /><span>{project?.name ?? '未关联项目'}</span><LockKeyhole size={11} /></div></div>
        <label className="ow-property-row"><span>Labels</span><input aria-label="任务标签" placeholder="添加标签，以逗号分隔" value={properties.labels} onChange={event => change('labels', event.target.value)} /></label>
        <p className="ow-property-note">Agent、项目和 cwd 在启动时确定，当前会话沿用这份执行上下文。</p>
      </section>
      <section className="ow-property-section"><h2>RELATIONSHIPS</h2>
        <label className="ow-property-row"><span>Parent</span><select aria-label="父任务" value={properties.parentId} onChange={event => change('parentId', event.target.value)}><option value="">None</option>{relationshipCandidates(task, tasks, 'parentId').map(item => <option key={item.id} value={item.id}>{item.id} · {item.title}</option>)}</select></label>
        <label className="ow-property-row"><span>Blocked by</span><select aria-label="阻塞此任务" value={properties.blockedBy} onChange={event => change('blockedBy', event.target.value)}><option value="">None</option>{relationshipCandidates(task, tasks, 'blockedBy').map(item => <option key={item.id} value={item.id}>{item.id} · {item.title}</option>)}</select></label>
        <div className="ow-property-row"><span>Blocking</span><div className="ow-property-links">{blocking.length ? blocking.map(item => <button key={item.id} onClick={() => onOpenTask(item.id)}>{item.id}<ArrowUpRight size={12} /></button>) : <span className="ow-muted">None</span>}</div></div>
        <div className="ow-property-row"><span>Subtasks</span><div className="ow-property-links">{subtasks.length ? subtasks.map(item => <button key={item.id} onClick={() => onOpenTask(item.id)}>{item.id}<ArrowUpRight size={12} /></button>) : <span className="ow-muted">None</span>}</div></div>
        <p className="ow-property-note">这里是任务之间的关系；会话的派生与依赖在 Workflow 中查看。</p>
      </section>
    </div><div>
      <section className="ow-property-section"><h2>EXECUTION</h2>{roles.map(role => <label className="ow-property-row ow-property-role" key={role.key}><span>{role.label}<small>{role.description}</small></span><select aria-label={role.label} disabled={workspace.phase !== 'execute'} value={properties[role.key]} onChange={event => change(role.key, event.target.value)}><option value="">None</option>{agents.map(item => <option key={item.id} value={item.id}>{item.name} · {item.role}</option>)}</select></label>)}<p className="ow-property-note">角色按需配置；进入审查后锁定本轮配置。Monitor / Watchdog 为本版的观察角色示意。</p></section>
      <section className="ow-property-section"><h2>ABOUT</h2><div className="ow-property-row"><span>Originating</span><div><Circle size={15} />{properties.origin}</div></div><div className="ow-property-row"><span>Created</span><time dateTime={properties.created}>{date(properties.created)}</time></div><div className="ow-property-row"><span>Updated</span><time dateTime={properties.updated}>{date(properties.updated)}</time></div></section>
    </div></div>
    <div className="ow-properties-cwd"><GitBranch size={15} /><span>{task.worktree ? '独立 worktree' : project?.repository ? '项目仓库' : '独立目录'}</span><code>{task.cwd}</code></div>
  </div>;
}
