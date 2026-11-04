import { t } from '../../lib/i18n';
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react';
import { Bot, FilePlus2, FolderOpen, GitFork, MessageSquarePlus, Plus, SlidersHorizontal, SquareTerminal, Workflow } from 'lucide-react';
import { TaskDetail } from './TaskDetail';
import { TaskProperties } from './TaskProperties';
import { TaskWorkflow } from './TaskWorkflow';
import { DocumentView, SessionTree, SessionView } from './SessionViews';
import { WorkspaceTabs } from './WorkspaceTabs';
import { Modal } from './ui';
import { closeWorkspaceTab, fixturePaths, newSession, openDocument, openSession, openWorkspaceTab, type Session, type TaskWorkspaceState } from './workspaceModel';
import type { Agent, Project, Task } from './model';
import './task-workspace.css';

type AddAction = 'acp' | 'terminal' | 'create-file' | 'open-file' | 'properties' | 'workflow';
type WorkspaceDialog = { kind: 'session'; parentId: string } | { kind: 'file'; create: boolean };

function AddMenu({ onAction, onClose }: { onAction: (action: AddAction) => void; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector('button')?.focus();
    const outside = (event: PointerEvent) => { if (!ref.current?.contains(event.target as Node) && !(event.target as Element).closest('.ow-add-tab')) onClose(); };
    document.addEventListener('pointerdown', outside);
    return () => document.removeEventListener('pointerdown', outside);
  }, [onClose]);
  const options: { id: AddAction; label: string; subtitle: string; icon: ReactNode }[] = [
    { id: 'acp', label: t('org_preview.new_acp_session'), subtitle: t('org_preview.new_acp_session_help'), icon: <MessageSquarePlus size={17} /> },
    { id: 'terminal', label: t('org_preview.new_terminal'), subtitle: t('org_preview.new_terminal_help'), icon: <SquareTerminal size={17} /> },
    { id: 'create-file', label: t('org_preview.new_file'), subtitle: t('org_preview.new_file_help'), icon: <FilePlus2 size={17} /> },
    { id: 'open-file', label: t('org_preview.open_file'), subtitle: t('org_preview.open_file_help'), icon: <FolderOpen size={17} /> },
    { id: 'properties', label: t('org_preview.properties'), subtitle: t('org_preview.properties_help'), icon: <SlidersHorizontal size={17} /> },
    { id: 'workflow', label: t('org_preview.workflow'), subtitle: t('org_preview.workflow_help'), icon: <Workflow size={17} /> },
  ];
  return <div ref={ref} className="ow-add-menu" role="menu" aria-label={t('org_preview.add_workspace_content')} onKeyDown={event => {
    if (event.key === 'Escape') { event.preventDefault(); onClose(); document.querySelector<HTMLButtonElement>('.ow-add-tab')?.focus(); }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); const items = Array.from(ref.current!.querySelectorAll('button')); const index = items.indexOf(document.activeElement as HTMLButtonElement);
      items[(index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    }
  }}><div className="ow-add-menu-heading">{t('org_preview.add_to_task')}</div>{options.map(option => <button key={option.id} role="menuitem" onClick={() => onAction(option.id)}>{option.icon}<span>{option.label}<small>{option.subtitle}</small></span></button>)}</div>;
}

export function TaskWorkspace({ task, project, agents, tasks, onUpdate, onShowFiles, onSelectTask }: {
  task: Task; project?: Project; agents: Agent[]; tasks: Task[]; onUpdate: (task: Task) => void; onShowFiles: () => void; onSelectTask: (id: string) => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const [dialog, setDialog] = useState<WorkspaceDialog | null>(null);
  const workspace = task.workspace!;
  const activeTab = workspace.tabs.find(item => item.id === workspace.activeTab) ?? workspace.tabs[0];
  const selectedSession = workspace.sessions.find(item => item.id === activeTab.target);
  const document = workspace.documents.find(item => item.path === activeTab.target);
  const change = (next: TaskWorkspaceState) => onUpdate({ ...task, workspace: next });
  const session = (id: string) => change(openSession(workspace, id));
  const special = (kind: 'properties' | 'workflow') => change(openWorkspaceTab(workspace, { id: kind, kind, target: kind, title: kind === 'properties' ? t('org_preview.properties') : t('org_preview.workflow') }));
  const updatePrimary = (next: Task) => {
    if (next.messages.length > task.messages.length) {
      next.workspace = { ...workspace, phase: 'execute', approved: false, sessions: workspace.sessions.map(item => item.id === 'main' ? { ...item, draft: '', status: 'running', ticks: 0 } : item.stage === 'review' || item.stage === 'approval' ? { ...item, stage: 'auxiliary', status: item.status === 'waiting' ? 'paused' : item.status } : item) };
    } else if (task.status === 'paused' && next.status !== 'paused') {
      next.workspace = { ...workspace, sessions: workspace.sessions.map(item => item.id === 'main' && item.status === 'paused' ? { ...item, status: 'running' } : item) };
    }
    onUpdate(next);
  };
  const addSession = (created: Session, waitForChild = false) => {
    const sessions = workspace.sessions.map(item => waitForChild && item.id === created.parentId ? { ...item, status: 'waiting' as const, dependsOn: [...item.dependsOn, created.id] } : item);
    change(openSession({ ...workspace, sessions: [...sessions, created] }, created.id)); setDialog(null);
  };
  const action = (selected: AddAction) => {
    setMenuOpen(false);
    if (selected === 'properties' || selected === 'workflow') special(selected);
    else if (selected === 'acp') setDialog({ kind: 'session', parentId: '' });
    else if (selected === 'terminal') addSession(newSession({ id: crypto.randomUUID(), title: t('org_preview.terminal_number', { number: workspace.sessions.filter(item => item.kind === 'terminal').length + 1 }), kind: 'terminal', agentId: '', goal: t('org_preview.terminal_goal') }));
    else setDialog({ kind: 'file', create: selected === 'create-file' });
  };
  const spawn = (parentId: string) => setDialog({ kind: 'session', parentId });
  return <div className="ow-task-workspace">
    <WorkspaceTabs workspace={workspace} onSelect={id => change({ ...workspace, activeTab: id })} onClose={id => change(closeWorkspaceTab(workspace, id))} onAdd={() => setMenuOpen(!menuOpen)} />
    {menuOpen && <AddMenu onAction={action} onClose={closeMenu} />}
    <div className="ow-workspace-toolbar"><span><span className="ow-live-dot" />{t('org_preview.session_count', { count: workspace.sessions.length })}<span className="ow-workspace-separator">/</span>{task.id}</span><div><button className={activeTab.kind === 'properties' ? 'active' : ''} onClick={() => special('properties')}><SlidersHorizontal size={13} />{t('org_preview.properties')}</button><button className={activeTab.kind === 'workflow' ? 'active' : ''} onClick={() => special('workflow')}><Workflow size={13} />{t('org_preview.workflow')}</button></div></div>
    <div id="ow-tab-content" role="tabpanel" aria-labelledby={`ow-tab-${workspace.tabs.indexOf(activeTab)}`} className="ow-tab-content">
      {activeTab.kind === 'main' && <TaskDetail task={task} agent={agents.find(item => item.id === task.agentId)} project={project} onUpdate={updatePrimary} onShowFiles={onShowFiles}
        draft={workspace.sessions[0].draft} onDraftChange={draft => change({ ...workspace, sessions: workspace.sessions.map(item => item.id === 'main' ? { ...item, draft } : item) })}
        collaboration={<><div className="ow-delegation-heading"><span><GitFork size={14} />{t('org_preview.main_session_coordination')}</span><button onClick={() => spawn('main')}><Plus size={13} />{t('org_preview.spawn_session')}</button></div><SessionTree workspace={workspace} agents={agents} onOpen={session} onSpawn={spawn} /></>} />}
      {activeTab.kind === 'session' && selectedSession && <SessionView task={task} session={selectedSession} agents={agents} onChange={changed => change({ ...workspace, sessions: workspace.sessions.map(item => item.id === changed.id ? changed : item) })} onOpen={session} onSpawn={spawn} onWorkflow={() => special('workflow')} />}
      {activeTab.kind === 'file' && document && <DocumentView key={document.path} path={document.path} content={document.content} saved={document.saved} cwd={task.cwd} onChange={content => change({ ...workspace, documents: workspace.documents.map(item => item.path === document.path ? { ...item, content } : item) })} onSave={() => change({ ...workspace, documents: workspace.documents.map(item => item.path === document.path ? { ...item, saved: item.content } : item) })} />}
      {activeTab.kind === 'properties' && <TaskProperties task={task} project={project} agents={agents} tasks={tasks} onChange={properties => change({ ...workspace, properties })} onTaskStatus={status => updatePrimary({ ...task, status })} onOpenTask={onSelectTask} onWorkflow={() => special('workflow')} />}
      {activeTab.kind === 'workflow' && <TaskWorkflow task={task} tasks={tasks} agents={agents} onOpen={session} onSpawn={spawn} onProperties={() => special('properties')} onApprove={() => onUpdate({ ...task, status: 'done', workspace: { ...workspace, approved: true, phase: 'done', sessions: workspace.sessions.map(item => item.stage === 'approval' ? { ...item, status: 'done' } : item) } })} />}
    </div>
    {dialog?.kind === 'session' && <SessionForm task={task} agents={agents} parentId={dialog.parentId} onClose={() => setDialog(null)} onCreate={addSession} />}
    {dialog?.kind === 'file' && <FilePicker task={task} repository={Boolean(project?.repository)} create={dialog.create} onClose={() => setDialog(null)} onOpen={path => { onUpdate(openDocument(task, path, dialog.create && !fixturePaths(task, Boolean(project?.repository)).includes(path))); setDialog(null); }} />}
  </div>;
}

function SessionForm({ task, agents, parentId, onClose, onCreate }: { task: Task; agents: Agent[]; parentId: string; onClose: () => void; onCreate: (session: Session, wait: boolean) => void }) {
  const [title, setTitle] = useState('');
  const [goal, setGoal] = useState('');
  const [agentId, setAgentId] = useState(task.agentId);
  const [dependency, setDependency] = useState('');
  const parent = task.workspace!.sessions.find(item => item.id === parentId);
  const workStage = task.workspace!.phase === 'execute' && parent?.stage === 'execute';
  const ancestors = new Set<string>();
  let current = parent;
  while (current) { ancestors.add(current.id); current = task.workspace!.sessions.find(item => item.id === current?.parentId); }
  // A parent may already wait for another descendant. Exclude any candidate
  // with a dependency path back to that parent to keep the new edge acyclic.
  const reachesAncestor = (id: string, visited = new Set<string>()): boolean => {
    if (ancestors.has(id)) return true;
    if (visited.has(id)) return false;
    visited.add(id);
    return task.workspace!.sessions.find(item => item.id === id)?.dependsOn.some(item => reachesAncestor(item, visited)) ?? false;
  };
  const choices = task.workspace!.sessions.filter(item => !reachesAncestor(item.id));
  return <Modal title={parent ? t('org_preview.spawn_session') : t('org_preview.new_acp_session')} onClose={onClose}><form className="ow-form" onSubmit={event => {
    event.preventDefault(); if (!title.trim() || !agentId) return;
    onCreate(newSession({ id: crypto.randomUUID(), title: title.trim(), agentId, parentId, dependsOn: dependency ? [dependency] : [], status: dependency ? 'waiting' : 'running', stage: workStage ? 'execute' : 'auxiliary', goal: goal.trim() || title.trim() }), Boolean(workStage));
  }}><p className="ow-help">{parent ? t(workStage ? 'org_preview.spawn_blocking' : 'org_preview.spawn_auxiliary', { name: parent.id === 'main' ? t('org_preview.main_session') : parent.title }) : t('org_preview.independent_session_help')}</p><label>{t('org_preview.session_name')}<input autoFocus required value={title} onChange={event => setTitle(event.target.value)} placeholder={t('org_preview.session_name_example')} /></label><label>{t('org_preview.assigned_agent')}<select aria-label={t('org_preview.session_agent')} value={agentId} onChange={event => setAgentId(event.target.value)} required>{agents.map(item => <option key={item.id} value={item.id}>{item.name} · {item.role}</option>)}</select></label><label>{t('org_preview.delegated_goal')}<textarea value={goal} onChange={event => setGoal(event.target.value)} placeholder={t('org_preview.session_goal_placeholder')} /></label><label>{t('org_preview.wait_for_session')} <span className="ow-muted">{t('org_preview.optional')}</span><select aria-label={t('org_preview.session_dependency')} value={dependency} onChange={event => setDependency(event.target.value)}><option value="">{t('org_preview.no_wait')}</option>{choices.map(item => <option key={item.id} value={item.id}>{item.id === 'main' ? t('org_preview.main_session') : item.title}</option>)}</select></label><div className="ow-form-cwd"><Bot size={14} /><code>{task.cwd}</code></div><div className="ow-form-actions"><button type="button" className="ow-secondary" onClick={onClose}>{t('org_preview.cancel')}</button><button className="ow-primary" disabled={!title.trim() || !agentId}>{t('org_preview.create_and_start_session')}</button></div></form></Modal>;
}

function FilePicker({ task, repository, create, onClose, onOpen }: { task: Task; repository: boolean; create: boolean; onClose: () => void; onOpen: (path: string) => void }) {
  const [path, setPath] = useState('');
  const paths = [...new Set([...fixturePaths(task, repository), ...task.workspace!.documents.map(item => item.path)])];
  const valid = Boolean(path.trim()) && !path.startsWith('/') && !path.includes('\\') && path.split('/').every(part => part.trim() && part !== '..' && part !== '.');
  return <Modal title={create ? t('org_preview.new_file') : t('org_preview.open_file')} onClose={onClose}>{create ? <form className="ow-form" onSubmit={event => { event.preventDefault(); if (valid) onOpen(path.trim()); }}><label>{t('org_preview.file_path')}<input autoFocus required value={path} onChange={event => setPath(event.target.value)} placeholder="notes/plan.md" /></label><p className="ow-help">{t('org_preview.file_path_help')}</p><div className="ow-form-cwd"><FolderOpen size={14} /><code>{task.cwd}</code></div><div className="ow-form-actions"><button className="ow-secondary" type="button" onClick={onClose}>{t('org_preview.cancel')}</button><button className="ow-primary" disabled={!valid}>{t('org_preview.create_file')}</button></div></form> : <div className="ow-file-picker"><label><span className="ow-sr-only">{t('org_preview.search_files')}</span><input autoFocus placeholder={t('org_preview.search_files_placeholder')} value={path} onChange={event => setPath(event.target.value)} /></label><code>{task.cwd}</code>{paths.filter(item => item.toLowerCase().includes(path.toLowerCase())).map(item => <button key={item} onClick={() => onOpen(item)}><FolderOpen size={14} />{item}</button>)}{!paths.some(item => item.toLowerCase().includes(path.toLowerCase())) && <p className="ow-help">{t('org_preview.no_matching_files')}</p>}</div>}</Modal>;
}
