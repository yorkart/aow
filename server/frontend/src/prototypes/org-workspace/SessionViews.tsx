import { t } from '../../lib/i18n';
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
      <button onClick={() => onOpen(session.id)}>{session.kind === 'terminal' ? <SquareTerminal size={15} /> : <Avatar agent={agents.find(item => item.id === session.agentId)} small />}<span>{session.id === 'main' ? t('org_preview.main_session') : session.title}<small>{session.id === 'main' ? t('org_preview.coordination_summary') : session.parentId ? t('org_preview.derived_from', { value0: workspace.sessions.find(item => item.id === session.parentId)?.title ?? t('org_preview.main_session') }) : t('org_preview.manual_auxiliary_session')}</small></span><span className={`ow-session-state ow-session-${session.status}`}>{session.stage === 'approval' && session.status === 'waiting' ? t('org_preview.awaiting_approval') : sessionLabels[session.status]}</span><ChevronRight size={12} /></button>
      <button className="ow-icon-button" aria-label={t('org_preview.spawn_from_session', { value0: session.id === 'main' ? t('org_preview.main_session') : session.title })} title={t('org_preview.spawn_session')} onClick={() => onSpawn(session.id)}><Plus size={13} /></button>
    </div>{rows(session.id, depth + 1)}
  </div>);
  return <div className="ow-session-tree"><div className="ow-session-tree-heading"><span><GitFork size={14} />{t('org_preview.session_collaboration')}</span><span>{t('org_preview.session_count', { count: workspace.sessions.length })}</span></div>{rows('', 0)}<div className="ow-session-tree-note">{t('org_preview.closed_tab_help')}</div></div>;
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
    const result = command === 'pwd' ? task.cwd : command === 'ls' ? 'task.md  result.md  frontend/' : command === 'git status' ? t('org_preview.simulated_git_status') : command === 'npm test' || command === 'npm run typecheck' ? t('org_preview.simulated_validation') : command === 'help' ? t('org_preview.simulated_terminal_help') : t('org_preview.unsupported_command', { value0: command });
    append(`$ ${command}\n${result}`);
  };
  return <div className="ow-session-view">
    <header className="ow-session-header"><div>{session.kind === 'terminal' ? <SquareTerminal size={20} /> : <Avatar agent={agent} />}<span><strong>{session.title}</strong><small>{session.kind === 'terminal' ? t('org_preview.simulated_shell') : `${agent?.name ?? 'Agent'} · ${agent?.role ?? t('org_preview.execution_role')} · ${agent?.connector ?? 'ACP'}`}</small></span></div><div><span className={`ow-session-state ow-session-${session.status}`}>{session.stage === 'approval' && session.status === 'waiting' ? t('org_preview.awaiting_approval') : sessionLabels[session.status]}</span><button className="ow-secondary" onClick={() => onSpawn(session.id)}><GitFork size={13} />{t('org_preview.spawn_session')}</button>{session.stage !== 'approval' && <button className="ow-icon-button" aria-label={session.status === 'paused' ? t('org_preview.resume_session') : t('org_preview.pause_session')} disabled={session.status === 'done'} onClick={() => onChange({ ...session, status: session.status === 'paused' ? 'running' : 'paused' })}>{session.status === 'paused' ? <Play size={14} /> : <Pause size={14} />}</button>}</div></header>
    <div className="ow-session-context"><span>{task.id}</span><span>/</span>{parent ? <button onClick={() => onOpen(parent.id)}><ArrowDownRight size={13} />{t('org_preview.parent_session', { name: parent.id === 'main' ? t('org_preview.main_session') : parent.title })}</button> : <span>{t('org_preview.manual_session')}</span>}<code title={task.cwd}>{task.cwd}</code></div>
    {session.dependsOn.length > 0 && <div className="ow-dependency-banner"><GitFork size={14} /><span>{waiting.length ? t('org_preview.waiting_for_dependencies') : t('org_preview.dependencies_ready')}</span>{session.dependsOn.map(id => <button key={id} onClick={() => onOpen(id)}>{task.workspace!.sessions.find(item => item.id === id)?.title}<ChevronRight size={12} /></button>)}</div>}
    {session.kind === 'terminal' ? <div className="ow-terminal"><div className="ow-terminal-heading"><span className="ow-live-dot" />zsh <span>·</span> {task.cwd}<span className="ow-small-label">{t('org_preview.simulated')}</span></div><pre>{session.output.join('\n') || t('org_preview.terminal_welcome')}</pre><form onSubmit={event => { event.preventDefault(); executeCommand(); }}><label htmlFor="ow-terminal-command">❯</label><input id="ow-terminal-command" aria-label={t('org_preview.terminal_command')} autoComplete="off" spellCheck={false} value={session.draft} onChange={event => onChange({ ...session, draft: event.target.value })} disabled={task.status === 'paused' || session.status === 'paused' || waiting.length > 0} placeholder={t('org_preview.terminal_command_placeholder')} /></form><div className="ow-terminal-note">{t('org_preview.terminal_demo_notice')}</div></div> : <>
      <div className="ow-session-conversation"><article className="ow-message ow-user-message"><div className="ow-message-heading"><MessageSquare size={14} /><strong>{parent ? t('org_preview.delegated_goal') : t('org_preview.session_goal')}</strong></div><p>{session.goal || t('org_preview.default_session_goal')}</p></article><article className="ow-message"><div className="ow-message-heading"><Avatar agent={agent} small /><strong>{agent?.name}</strong></div><p>{session.status === 'done' ? t('org_preview.session_completed_reply') : waiting.length ? t('org_preview.session_waiting_reply') : t('org_preview.session_started_reply')}</p>{session.stage === 'approval' && <button className="ow-secondary ow-approval-link" onClick={onWorkflow}>{t('org_preview.view_approval_node')}<ChevronRight size={13} /></button>}</article>{session.messages.map((message, index) => <article className="ow-message ow-user-message" key={index}><p>{message}</p></article>)}{session.status === 'done' && <div className="ow-session-result"><Check size={16} />{t('org_preview.demo_result_ready')}{parent && <button onClick={() => onOpen(parent.id)}>{t('org_preview.back_to_parent')}<ChevronRight size={12} /></button>}</div>}</div>
      <form className="ow-session-composer" onSubmit={event => { event.preventDefault(); if (!session.draft.trim()) return; onChange({ ...session, messages: [...session.messages, session.draft.trim()], draft: '' }); }}><input aria-label={t('org_preview.child_session_message')} placeholder={t('org_preview.session_message_placeholder')} value={session.draft} onChange={event => onChange({ ...session, draft: event.target.value })} /><button className="ow-send" aria-label={t('org_preview.send_session_message')} disabled={!session.draft.trim()}><ArrowUp size={16} /></button></form>
    </>}
  </div>;
}

export function DocumentView({ path, content, saved, cwd, onChange, onSave }: { path: string; content: string; saved: string; cwd: string; onChange: (text: string) => void; onSave: () => void }) {
  const [notice, setNotice] = useState('');
  const save = () => { onSave(); setNotice(t('org_preview.saved_to_demo_workspace')); };
  return <div className="ow-document-view"><header><span>{path}</span><small>{content === saved ? t('org_preview.saved') : t('org_preview.unsaved')}</small><button className="ow-secondary" onClick={save}><Check size={13} />{t('org_preview.save')}</button></header><div className="ow-editor-surface"><pre aria-hidden="true">{content.split('\n').map((_, index) => index + 1).join('\n')}</pre><textarea spellCheck={false} aria-label={t('org_preview.edit_file', { value0: path })} value={content} onChange={event => { setNotice(''); onChange(event.target.value); }} onKeyDown={event => { if ((event.metaKey || event.ctrlKey) && event.key === 's') { event.preventDefault(); save(); } }} /></div><footer><code>{cwd}/{path}</code><span role="status">{notice || t('org_preview.demo_editor_notice')}</span></footer></div>;
}
