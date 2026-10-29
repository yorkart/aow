import type { ReactNode } from 'react';
import { ArrowUp, Check, ChevronDown, FileCode2, GitBranch, Pause, Play, SquareTerminal } from 'lucide-react';
import { Avatar, Status } from './ui';
import type { Agent, Project, Task } from './model';

export function TaskDetail({ task, agent, project, onUpdate, onShowFiles, collaboration, draft, onDraftChange }: {
  task: Task; agent?: Agent; project?: Project; onUpdate: (task: Task) => void; onShowFiles: () => void;
  collaboration: ReactNode; draft: string; onDraftChange: (text: string) => void;
}) {
  const steps = ['理解任务与工作上下文', project?.repository ? '检查代码并实现改动' : '整理信息并生成内容', '验证结果并整理交付'];
  return <div className="ow-task-detail">
    <div className="ow-task-scroll"><div className="ow-task-content">
      <div className="ow-task-eyebrow"><span>{task.id}</span><Status status={task.status} /><span className="ow-small-label">演示运行</span></div>
      <h1>{task.title}</h1>
      <div className="ow-task-meta"><span><Avatar agent={agent} small />{agent?.name}<span className="ow-muted">{agent?.role}</span></span><span><GitBranch size={14} />{task.worktree ? `task/${task.id.toLowerCase()}` : project?.repository ? 'main' : '独立任务目录'}</span></div>
      <div className="ow-primary-collaboration">{collaboration}</div>
      <article className="ow-message ow-user-message"><div className="ow-message-heading"><span className="ow-user-avatar">Y</span><strong>你</strong><time>{task.time}</time></div><p>{task.prompt}</p></article>
      <article className="ow-message"><div className="ow-message-heading"><Avatar agent={agent} small /><strong>{agent?.name}</strong><span className="ow-role-label">{agent?.role}</span></div>
        <p>收到。我会先理解{project ? <><strong>{project.name}</strong> 的上下文</> : '这项任务的目标'}，然后逐步完成任务，并把过程和结果留在这里。</p>
        <div className="ow-plan"><div className="ow-plan-title"><span>执行计划</span><span>{Math.min(task.progress, 3)} / 3</span></div>{steps.map((step, index) => <div className={index < task.progress ? 'ow-step ow-step-done' : 'ow-step'} key={step}><span>{index < task.progress ? <Check size={12} /> : index + 1}</span>{step}{index === task.progress && task.status === 'running' && <span className="ow-step-current">进行中</span>}</div>)}</div>
        <details className="ow-execution"><summary><SquareTerminal size={15} /><span>执行记录</span><span className="ow-muted">{task.progress + 1} 个步骤</span><ChevronDown size={13} /></summary><div><code>$ cd {task.cwd}</code><code>✓ 工作目录已就绪（模拟）</code><code>✓ 通过 {agent?.connector} 连接 {agent?.name}（模拟）</code>{task.progress > 0 && <code>✓ 已读取任务上下文</code>}{task.progress > 1 && <code>✓ 已生成演示产物</code>}{task.progress > 2 && <code>✓ 演示运行结束，等待查看结果</code>}</div></details>
        {task.progress > 1 && <><p>{project?.repository ? '已准备好一组演示改动，可以在右侧查看文件与变更。' : '已将任务结果整理到独立工作目录，可以在右侧查看文档。'}</p><button className="ow-artifact" onClick={onShowFiles}><FileCode2 size={19} /><span><strong>{project?.repository ? '查看任务改动' : '查看任务文档'}</strong><small>{project?.name ?? '独立任务'} · {project?.repository ? '3 个文件' : '任务产物'}</small></span><span>→</span></button></>}
      </article>
      {task.messages.map((text, index) => <article className="ow-message ow-user-message" key={`${index}-${text}`}><div className="ow-message-heading"><span className="ow-user-avatar">Y</span><strong>你</strong></div><p>{text}</p><small className="ow-muted">已加入本次演示任务，继续模拟执行。</small></article>)}
      <div className="ow-run-state" role="status"><Status status={task.status} /><span>{task.status === 'running' ? `${agent?.name} 正在处理这项任务…` : task.status === 'paused' ? '可以继续执行，或补充任务要求。' : '本次演示执行已结束，可以查看结果或继续补充。'}</span>{(task.status === 'running' || task.status === 'paused') && <button onClick={() => onUpdate({ ...task, status: task.status === 'running' ? 'paused' : 'running' })}>{task.status === 'running' ? <><Pause size={13} />暂停</> : <><Play size={13} />继续</>}</button>}</div>
    </div></div>
    <div className="ow-followup-wrap"><form className="ow-followup" onSubmit={event => { event.preventDefault(); if (!draft.trim()) return; onUpdate({ ...task, messages: [...task.messages, draft.trim()], status: 'running', progress: 1 }); }}><label className="ow-sr-only" htmlFor="ow-followup">补充任务要求</label><input id="ow-followup" placeholder="补充要求，或一起讨论下一步…" value={draft} onChange={event => onDraftChange(event.target.value)} /><button className="ow-send" type="submit" disabled={!draft.trim()} aria-label="发送补充要求"><ArrowUp size={17} /></button></form><div className="ow-followup-caption"><span><Avatar agent={agent} small />{agent?.name} · {agent?.connector}</span><span>主会话 · 汇总当前任务的协作结果</span></div></div>
  </div>;
}
