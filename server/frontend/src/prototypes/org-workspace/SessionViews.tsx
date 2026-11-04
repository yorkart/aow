import { useState } from 'react';
import { ArrowDownRight, ArrowUp, Check, ChevronRight, CornerDownRight, GitFork, MessageSquare, Pause, Play, Plus, SquareTerminal } from 'lucide-react';
import { Avatar } from './ui';
import { sessionLabels, type Session, type TaskWorkspaceState } from './workspaceModel';
import type { Agent, Task } from './model';

export function SessionTree({ workspace, agents, onOpen, onSpawn }: {
  workspace: TaskWorkspaceState; agents: Agent[]; onOpen: (id: string) => void; onSpawn: (parentId: string) => void;
}) {
  const rows = (parentId: string, depth: number) => workspace.sessions.filter(item => item.parentId === parentId).map(session => <div key={session.id}>
    <div className="ow-session-tree-row" style={{ paddingLeft: 12 + depth * 18 }}>
      {depth > 0 && <CornerDownRight size={13} />}
      <button onClick={() => onOpen(session.id)}>{session.kind === 'terminal' ? <SquareTerminal size={15} /> : <Avatar agent={agents.find(item => item.id === session.agentId)} small />}<span>{session.id === 'main' ? '主会话' : session.title}<small>{session.id === 'main' ? '协调与汇总' : session.parentId ? `由 ${workspace.sessions.find(item => item.id === session.parentId)?.title ?? '主会话'} 派生` : '手动创建 · 独立辅助会话'}</small></span><span className={`ow-session-state ow-session-${session.status}`}>{session.stage === 'approval' && session.status === 'waiting' ? '等待批准' : sessionLabels[session.status]}</span><ChevronRight size={12} /></button>
      <button className="ow-icon-button" aria-label={`从 ${session.id === 'main' ? '主会话' : session.title} 派生子会话`} title="派生子会话" onClick={() => onSpawn(session.id)}><Plus size={13} /></button>
    </div>{rows(session.id, depth + 1)}
  </div>);
  return <div className="ow-session-tree"><div className="ow-session-tree-heading"><span><GitFork size={14} />会话协作</span><span>{workspace.sessions.length} 个会话</span></div>{rows('', 0)}<div className="ow-session-tree-note">关闭 Tab 后，会话仍保留在这里。</div></div>;
}

export function SessionView({ task, session, agents, onChange, onOpen, onSpawn, onWorkflow }: {
  task: Task; session: Session; agents: Agent[]; onChange: (session: Session) => void; onOpen: (id: string) => void; onSpawn: (id: string) => void; onWorkflow: () => void;
}) {
  const agent = agents.find(item => item.id === session.agentId);
  const parent = task.workspace!.sessions.find(item => item.id === session.parentId);
  const waiting = session.dependsOn.filter(id => task.workspace!.sessions.find(item => item.id === id)?.status !== 'done');
  const append = (text: string) => onChange({ ...session, draft: '', output: [...session.output, ...text.split('\n')] });
  const executeCommand = () => {
    const command = session.draft.trim();
    if (!command) return;
    if (command === 'clear') { onChange({ ...session, draft: '', output: [] }); return; }
    const result = command === 'pwd' ? task.cwd : command === 'ls' ? 'task.md  result.md  frontend/' : command === 'git status' ? '模拟工作区：3 个文件有改动；尚未提交。' : command === 'npm test' || command === 'npm run typecheck' ? '模拟验证完成 · 此输出不代表真实测试结果。' : command === 'help' ? '演示命令：pwd · ls · git status · npm test · npm run typecheck · clear\n命令只在内存中模拟，不调用 shell。' : `演示 Terminal 不执行真实命令：${command}\n输入 help 查看支持的交互。`;
    append(`$ ${command}\n${result}`);
  };
  return <div className="ow-session-view">
    <header className="ow-session-header"><div>{session.kind === 'terminal' ? <SquareTerminal size={20} /> : <Avatar agent={agent} />}<span><strong>{session.title}</strong><small>{session.kind === 'terminal' ? 'Terminal · 模拟 shell' : `${agent?.name ?? 'Agent'} · ${agent?.role ?? '执行角色'} · ${agent?.connector ?? 'ACP'}`}</small></span></div><div><span className={`ow-session-state ow-session-${session.status}`}>{session.stage === 'approval' && session.status === 'waiting' ? '等待批准' : sessionLabels[session.status]}</span><button className="ow-secondary" onClick={() => onSpawn(session.id)}><GitFork size={13} />派生子会话</button>{session.stage !== 'approval' && <button className="ow-icon-button" aria-label={session.status === 'paused' ? '继续此会话' : '暂停此会话'} disabled={session.status === 'done'} onClick={() => onChange({ ...session, status: session.status === 'paused' ? 'running' : 'paused' })}>{session.status === 'paused' ? <Play size={14} /> : <Pause size={14} />}</button>}</div></header>
    <div className="ow-session-context"><span>{task.id}</span><span>/</span>{parent ? <button onClick={() => onOpen(parent.id)}><ArrowDownRight size={13} />父会话：{parent.id === 'main' ? '主会话' : parent.title}</button> : <span>手动创建的辅助会话</span>}<code title={task.cwd}>{task.cwd}</code></div>
    {session.dependsOn.length > 0 && <div className="ow-dependency-banner"><GitFork size={14} /><span>{waiting.length ? '等待依赖完成' : '依赖已满足'}</span>{session.dependsOn.map(id => <button key={id} onClick={() => onOpen(id)}>{task.workspace!.sessions.find(item => item.id === id)?.title}<ChevronRight size={12} /></button>)}</div>}
    {session.kind === 'terminal' ? <div className="ow-terminal"><div className="ow-terminal-heading"><span className="ow-live-dot" />zsh <span>·</span> {task.cwd}<span className="ow-small-label">SIMULATED</span></div><pre>{session.output.join('\n') || 'AoW Task Terminal\n输入 help 查看可用演示命令。'}</pre><form onSubmit={event => { event.preventDefault(); executeCommand(); }}><label htmlFor="ow-terminal-command">❯</label><input id="ow-terminal-command" aria-label="Terminal 命令" autoComplete="off" spellCheck={false} value={session.draft} onChange={event => onChange({ ...session, draft: event.target.value })} disabled={task.status === 'paused' || session.status === 'paused' || waiting.length > 0} placeholder="输入命令，例如 pwd" /></form><div className="ow-terminal-note">当前 task 的 cwd · 演示命令不会在系统中执行</div></div> : <>
      <div className="ow-session-conversation"><article className="ow-message ow-user-message"><div className="ow-message-heading"><MessageSquare size={14} /><strong>{parent ? '委派目标' : '会话目标'}</strong></div><p>{session.goal || '在当前任务的工作目录中协助处理问题。'}</p></article><article className="ow-message"><div className="ow-message-heading"><Avatar agent={agent} small /><strong>{agent?.name}</strong></div><p>{session.status === 'done' ? '这部分工作已完成，结果已回传到所属任务。' : waiting.length ? '我会等待前置会话完成，然后使用同一任务的工作目录继续。' : '已接收任务上下文。我会在当前会话中完成这部分工作，并保留独立的过程记录。'}</p>{session.stage === 'approval' && <button className="ow-secondary ow-approval-link" onClick={onWorkflow}>在 Workflow 中查看批准节点<ChevronRight size={13} /></button>}</article>{session.messages.map((message, index) => <article className="ow-message ow-user-message" key={index}><p>{message}</p></article>)}{session.status === 'done' && <div className="ow-session-result"><Check size={16} />演示结果已就绪{parent && <button onClick={() => onOpen(parent.id)}>返回父会话<ChevronRight size={12} /></button>}</div>}</div>
      <form className="ow-session-composer" onSubmit={event => { event.preventDefault(); if (!session.draft.trim()) return; onChange({ ...session, messages: [...session.messages, session.draft.trim()], draft: '' }); }}><input aria-label="子会话消息" placeholder="在这个会话中继续讨论…" value={session.draft} onChange={event => onChange({ ...session, draft: event.target.value })} /><button className="ow-send" aria-label="发送会话消息" disabled={!session.draft.trim()}><ArrowUp size={16} /></button></form>
    </>}
  </div>;
}

export function DocumentView({ path, content, saved, cwd, onChange, onSave }: { path: string; content: string; saved: string; cwd: string; onChange: (text: string) => void; onSave: () => void }) {
  const [notice, setNotice] = useState('');
  const save = () => { onSave(); setNotice('已保存到演示工作区'); };
  return <div className="ow-document-view"><header><span>{path}</span><small>{content === saved ? '已保存' : '未保存'}</small><button className="ow-secondary" onClick={save}><Check size={13} />保存</button></header><div className="ow-editor-surface"><pre aria-hidden="true">{content.split('\n').map((_, index) => index + 1).join('\n')}</pre><textarea spellCheck={false} aria-label={`编辑 ${path}`} value={content} onChange={event => { setNotice(''); onChange(event.target.value); }} onKeyDown={event => { if ((event.metaKey || event.ctrlKey) && event.key === 's') { event.preventDefault(); save(); } }} /></div><footer><code>{cwd}/{path}</code><span role="status">{notice || '演示编辑器 · 修改保留在当前任务内存中'}</span></footer></div>;
}
