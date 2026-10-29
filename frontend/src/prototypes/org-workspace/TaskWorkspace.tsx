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
    { id: 'acp', label: '新建 ACP 会话', subtitle: '选择 Agent，协作处理任务', icon: <MessageSquarePlus size={17} /> },
    { id: 'terminal', label: '新建 Terminal', subtitle: '在当前任务目录打开终端', icon: <SquareTerminal size={17} /> },
    { id: 'create-file', label: '新建文件', subtitle: '创建任务文档或代码', icon: <FilePlus2 size={17} /> },
    { id: 'open-file', label: '打开文件', subtitle: '浏览当前任务的工作目录', icon: <FolderOpen size={17} /> },
    { id: 'properties', label: 'Properties', subtitle: '任务属性、关系与执行角色', icon: <SlidersHorizontal size={17} /> },
    { id: 'workflow', label: 'Workflow', subtitle: '会话层级与执行依赖', icon: <Workflow size={17} /> },
  ];
  return <div ref={ref} className="ow-add-menu" role="menu" aria-label="添加工作区内容" onKeyDown={event => {
    if (event.key === 'Escape') { event.preventDefault(); onClose(); document.querySelector<HTMLButtonElement>('.ow-add-tab')?.focus(); }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault(); const items = Array.from(ref.current!.querySelectorAll('button')); const index = items.indexOf(document.activeElement as HTMLButtonElement);
      items[(index + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    }
  }}><div className="ow-add-menu-heading">添加到当前 Task</div>{options.map(option => <button key={option.id} role="menuitem" onClick={() => onAction(option.id)}>{option.icon}<span>{option.label}<small>{option.subtitle}</small></span></button>)}</div>;
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
  const special = (kind: 'properties' | 'workflow') => change(openWorkspaceTab(workspace, { id: kind, kind, target: kind, title: kind === 'properties' ? 'Properties' : 'Workflow' }));
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
    else if (selected === 'terminal') addSession(newSession({ id: crypto.randomUUID(), title: `Terminal ${workspace.sessions.filter(item => item.kind === 'terminal').length + 1}`, kind: 'terminal', agentId: '', goal: '在当前 task 的 cwd 中运行命令。' }));
    else setDialog({ kind: 'file', create: selected === 'create-file' });
  };
  const spawn = (parentId: string) => setDialog({ kind: 'session', parentId });
  return <div className="ow-task-workspace">
    <WorkspaceTabs workspace={workspace} onSelect={id => change({ ...workspace, activeTab: id })} onClose={id => change(closeWorkspaceTab(workspace, id))} onAdd={() => setMenuOpen(!menuOpen)} />
    {menuOpen && <AddMenu onAction={action} onClose={closeMenu} />}
    <div className="ow-workspace-toolbar"><span><span className="ow-live-dot" />{workspace.sessions.length} 个会话<span className="ow-workspace-separator">/</span>{task.id}</span><div><button className={activeTab.kind === 'properties' ? 'active' : ''} onClick={() => special('properties')}><SlidersHorizontal size={13} />Properties</button><button className={activeTab.kind === 'workflow' ? 'active' : ''} onClick={() => special('workflow')}><Workflow size={13} />Workflow</button></div></div>
    <div id="ow-tab-content" role="tabpanel" aria-labelledby={`ow-tab-${workspace.tabs.indexOf(activeTab)}`} className="ow-tab-content">
      {activeTab.kind === 'main' && <TaskDetail task={task} agent={agents.find(item => item.id === task.agentId)} project={project} onUpdate={updatePrimary} onShowFiles={onShowFiles}
        draft={workspace.sessions[0].draft} onDraftChange={draft => change({ ...workspace, sessions: workspace.sessions.map(item => item.id === 'main' ? { ...item, draft } : item) })}
        collaboration={<><div className="ow-delegation-heading"><span><GitFork size={14} />主会话协调与汇总</span><button onClick={() => spawn('main')}><Plus size={13} />派生子会话</button></div><SessionTree workspace={workspace} agents={agents} onOpen={session} onSpawn={spawn} /></>} />}
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
  return <Modal title={parent ? '派生子会话' : '新建 ACP 会话'} onClose={onClose}><form className="ow-form" onSubmit={event => {
    event.preventDefault(); if (!title.trim() || !agentId) return;
    onCreate(newSession({ id: crypto.randomUUID(), title: title.trim(), agentId, parentId, dependsOn: dependency ? [dependency] : [], status: dependency ? 'waiting' : 'running', stage: workStage ? 'execute' : 'auxiliary', goal: goal.trim() || title.trim() }), Boolean(workStage));
  }}><p className="ow-help">{parent ? `由「${parent.id === 'main' ? '主会话' : parent.title}」派生${workStage ? '，父会话将等待结果' : '，作为后续辅助会话'}。` : '手动创建独立辅助会话，与主会话共享当前任务目录。'}</p><label>会话名称<input autoFocus required value={title} onChange={event => setTitle(event.target.value)} placeholder="例如：审查边界条件" /></label><label>执行 Agent<select aria-label="会话执行 Agent" value={agentId} onChange={event => setAgentId(event.target.value)} required>{agents.map(item => <option key={item.id} value={item.id}>{item.name} · {item.role}</option>)}</select></label><label>委派目标<textarea value={goal} onChange={event => setGoal(event.target.value)} placeholder="这部分工作需要完成什么？" /></label><label>等待会话 <span className="ow-muted">可选</span><select aria-label="会话执行依赖" value={dependency} onChange={event => setDependency(event.target.value)}><option value="">无需等待</option>{choices.map(item => <option key={item.id} value={item.id}>{item.id === 'main' ? '主会话' : item.title}</option>)}</select></label><div className="ow-form-cwd"><Bot size={14} /><code>{task.cwd}</code></div><div className="ow-form-actions"><button type="button" className="ow-secondary" onClick={onClose}>取消</button><button className="ow-primary" disabled={!title.trim() || !agentId}>创建并启动会话</button></div></form></Modal>;
}

function FilePicker({ task, repository, create, onClose, onOpen }: { task: Task; repository: boolean; create: boolean; onClose: () => void; onOpen: (path: string) => void }) {
  const [path, setPath] = useState('');
  const paths = [...new Set([...fixturePaths(task, repository), ...task.workspace!.documents.map(item => item.path)])];
  const valid = Boolean(path.trim()) && !path.startsWith('/') && !path.includes('\\') && path.split('/').every(part => part.trim() && part !== '..' && part !== '.');
  return <Modal title={create ? '新建文件' : '打开文件'} onClose={onClose}>{create ? <form className="ow-form" onSubmit={event => { event.preventDefault(); if (valid) onOpen(path.trim()); }}><label>文件路径<input autoFocus required value={path} onChange={event => setPath(event.target.value)} placeholder="notes/plan.md" /></label><p className="ow-help">相对于当前 task 的 cwd。已有文件会直接打开，编辑内容保留在演示工作区。</p><div className="ow-form-cwd"><FolderOpen size={14} /><code>{task.cwd}</code></div><div className="ow-form-actions"><button className="ow-secondary" type="button" onClick={onClose}>取消</button><button className="ow-primary" disabled={!valid}>创建文件</button></div></form> : <div className="ow-file-picker"><label><span className="ow-sr-only">搜索文件</span><input autoFocus placeholder="搜索工作区文件…" value={path} onChange={event => setPath(event.target.value)} /></label><code>{task.cwd}</code>{paths.filter(item => item.toLowerCase().includes(path.toLowerCase())).map(item => <button key={item} onClick={() => onOpen(item)}><FolderOpen size={14} />{item}</button>)}{!paths.some(item => item.toLowerCase().includes(path.toLowerCase())) && <p className="ow-help">没有匹配的文件</p>}</div>}</Modal>;
}
