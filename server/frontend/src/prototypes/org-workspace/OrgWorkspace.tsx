import { useEffect, useState } from 'react';
import { ArrowUpRight, Bot, Check, ChevronDown, ChevronRight, CircleHelp, Folder, Layers3, ListTodo, PanelLeftClose, PanelLeftOpen, PanelRightOpen, Plus, Search, SlidersHorizontal, Sparkles, Zap } from 'lucide-react';
import { ContextPanel, type ContextView } from './ContextPanel';
import { AgentForm, OrgForm, ProjectForm, ResourceView } from './Resources';
import { TaskComposer } from './TaskComposer';
import { TaskWorkspace } from './TaskWorkspace';
import { advanceTask, openDocument, prepareTask } from './workspaceModel';
import { Avatar, Modal, Status } from './ui';
import { demoAgents, demoAutomations, demoConnectors, demoOrgs, demoProjects, demoTasks, sectionLabels, taskDirectory, type Agent, type Org, type Project, type Section, type Task, type TaskStatus } from './model';
import './workspace.css';

type Dialog = { kind: 'org' | 'switcher' | 'help' } | { kind: 'project'; project?: Project } | { kind: 'agent'; agent?: Agent };
const navigation = [{ id: 'tasks', icon: ListTodo }, { id: 'projects', icon: Folder }, { id: 'agents', icon: Bot }, { id: 'automation', icon: Zap }] as const;

export default function OrgWorkspace() {
  const [orgs, setOrgs] = useState(demoOrgs);
  const [orgId, setOrgId] = useState(demoOrgs[0].id);
  const [projects, setProjects] = useState(demoProjects);
  const [agents, setAgents] = useState(demoAgents);
  const [tasks, setTasks] = useState(() => demoTasks.map(prepareTask));
  const [connectors, setConnectors] = useState(demoConnectors);
  const [automations, setAutomations] = useState(demoAutomations);
  const [section, setSection] = useState<Section>('tasks');
  const [taskId, setTaskId] = useState<string | null>(demoTasks[0].id);
  const [initialProject, setInitialProject] = useState('');
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState('all');
  const [contextView, setContextView] = useState<ContextView>('explorer');
  const [contextOpen, setContextOpen] = useState(true);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const org = orgs.find(item => item.id === orgId)!;
  const orgProjects = projects.filter(item => item.orgId === orgId);
  const orgAgents = agents.filter(item => item.orgId === orgId);
  const orgTasks = tasks.filter(item => item.orgId === orgId);
  const orgAutomations = automations.filter(item => item.orgId === orgId);
  const task = orgTasks.find(item => item.id === taskId);
  const project = orgProjects.find(item => item.id === task?.projectId);
  const nextId = `AOW-${129 + tasks.length - demoTasks.length}`;
  const runningCount = orgTasks.filter(item => item.status === 'running').length;
  const filteredTasks = orgTasks.filter(item => (filter === 'all' || item.status === filter) && `${item.title} ${item.id} ${orgProjects.find(project => project.id === item.projectId)?.name ?? ''}`.toLowerCase().includes(query.toLowerCase()));

  // The preview never starts a process or contacts the backend. Its clock only
  // advances in-memory task records, including runs in a different organization.
  useEffect(() => {
    const timer = window.setInterval(() => setTasks(current => current.map(item => advanceTask(item, current))), 6000);
    return () => window.clearInterval(timer);
  }, []);

  const revealWorkspace = () => { if (window.matchMedia('(max-width: 760px)').matches) setSidebarOpen(false); };
  const newTask = (projectId = '') => { revealWorkspace(); setSection('tasks'); setTaskId(null); setInitialProject(projectId); setQuery(''); setFilter('all'); };
  const createTask = (created: Task) => { setTasks(current => [prepareTask(created), ...current]); setTaskId(created.id); setSection('tasks'); setFilter('all'); setQuery(''); };
  const selectOrg = (selected: Org) => { setOrgId(selected.id); setTaskId(null); setInitialProject(''); setSection('tasks'); setQuery(''); setFilter('all'); setDialog(null); };
  const updateTask = (updated: Task) => setTasks(current => current.map(item => item.id === updated.id ? updated : item));
  const addResource = () => setDialog(section === 'projects' ? { kind: 'project' } : { kind: 'agent' });
  const visibleContext = Boolean(task && section === 'tasks' && contextOpen);

  return <div className={`ow-app ${!sidebarOpen ? 'ow-sidebar-hidden' : ''} ${visibleContext ? 'ow-has-context' : ''}`}>
    <nav className="ow-rail" aria-label="功能导航"><button className="ow-brand" title="新建任务" aria-label="返回任务首页" onClick={() => newTask()}><Layers3 size={22} /></button><div className="ow-rail-items">{navigation.map(item => <button className={section === item.id ? 'active' : ''} aria-label={sectionLabels[item.id]} aria-current={section === item.id ? 'page' : undefined} key={item.id} onClick={() => { setSection(item.id); revealWorkspace(); }}><item.icon size={20} /><span>{sectionLabels[item.id]}</span></button>)}</div><div className="ow-rail-bottom"><button aria-label="关于设计预览" onClick={() => setDialog({ kind: 'help' })}><CircleHelp size={19} /></button><button className="ow-account" aria-label="切换组织" onClick={() => setDialog({ kind: 'switcher' })}>{org.initials}</button></div></nav>
    {sidebarOpen && <aside className="ow-sidebar" aria-label={`${sectionLabels[section]} 列表`}>
      <header className="ow-org-header"><button onClick={() => setDialog({ kind: 'switcher' })} aria-label={`切换组织，当前 ${org.name}`}><span className="ow-org-icon">{org.initials}</span><strong>{org.name}</strong><ChevronDown size={14} /></button><button className="ow-icon-button" aria-label="收起列表" onClick={() => setSidebarOpen(false)}><PanelLeftClose size={16} /></button></header>
      <div className="ow-sidebar-main-action"><button onClick={() => section === 'tasks' || section === 'automation' ? newTask() : addResource()}><Plus size={17} />{section === 'tasks' || section === 'automation' ? '新建任务' : section === 'projects' ? '新建项目' : '新建 Agent'}<span aria-hidden="true">＋</span></button></div>
      {section === 'tasks' ? <><label className="ow-search"><Search size={14} /><input aria-label="搜索任务" placeholder="搜索任务…" value={query} onChange={event => setQuery(event.target.value)} /></label><div className="ow-list-heading"><span>Tasks <small>{orgTasks.length}</small></span><label><SlidersHorizontal size={12} /><select aria-label="任务状态筛选" value={filter} onChange={event => setFilter(event.target.value)}><option value="all">全部</option><option value="running">执行中</option><option value="review">待检查</option><option value="done">已完成</option><option value="paused">已暂停</option></select></label></div><div className="ow-task-list">{([{ title: '进行中', statuses: ['running', 'review'] }, { title: '最近任务', statuses: ['done', 'paused'] }] as { title: string; statuses: TaskStatus[] }[]).map(group => { const entries = filteredTasks.filter(item => group.statuses.includes(item.status)); return entries.length > 0 && <div className="ow-task-group" key={group.title}><h2>{group.title}<span>{entries.length}</span></h2>{entries.map(item => { const itemProject = orgProjects.find(project => project.id === item.projectId); return <button key={item.id} className={`ow-task-row ${taskId === item.id ? 'active' : ''}`} onClick={() => { setTaskId(item.id); revealWorkspace(); }} aria-current={taskId === item.id ? 'true' : undefined}><Status status={item.status} label={false} /><span><strong>{item.title}</strong><small><span>{itemProject?.name ?? '未关联项目'}</span><span>·</span>{item.time}</small></span></button>; })}</div>; })}{!filteredTasks.length && <p className="ow-list-empty">{query || filter !== 'all' ? '没有匹配的任务' : '第一个任务，从这里开始。'}</p>}</div></> : <div className="ow-resource-nav"><div className="ow-list-heading">{sectionLabels[section]}<small>{section === 'projects' ? orgProjects.length : section === 'agents' ? orgAgents.length : orgAutomations.length}</small></div>{section === 'projects' && orgProjects.map(item => <button key={item.id} onClick={() => setDialog({ kind: 'project', project: item })}><span className={`ow-project-dot ow-${item.color}`} /><span>{item.name}</span><ChevronRight size={13} /></button>)}{section === 'agents' && orgAgents.map(item => <button key={item.id} onClick={() => setDialog({ kind: 'agent', agent: item })}><Avatar agent={item} small /><span>{item.name}<small>{item.role}</small></span><ChevronRight size={13} /></button>)}{section === 'automation' && <p className="ow-help">周期性工作自动成为任务，执行过程统一在 Tasks 中查看。</p>}</div>}
      <footer className="ow-sidebar-footer"><span className="ow-live-dot" /><span>{runningCount ? `${runningCount} 个任务正在执行` : '一切就绪，开始下一项任务'}</span><span className="ow-small-label">DEMO</span></footer>
    </aside>}
    <main className="ow-main"><header className="ow-main-header"><div>{!sidebarOpen && <button className="ow-icon-button" aria-label="展开列表" onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={16} /></button>}<span>{sectionLabels[section]}</span><ChevronRight size={12} /><strong>{section === 'tasks' ? task ? project?.name ?? '独立任务' : '新建任务' : org.name}</strong></div><div><span className="ow-preview-badge"><span />设计预览</span>{task && section === 'tasks' && <button className="ow-icon-button" aria-label={contextOpen ? '隐藏工作上下文' : '显示工作上下文'} aria-pressed={contextOpen} onClick={() => setContextOpen(!contextOpen)}><PanelRightOpen size={17} /></button>}</div></header>
      {section === 'tasks' ? task ? <TaskWorkspace key={task.id} task={task} agents={orgAgents} tasks={orgTasks} project={project} onSelectTask={setTaskId} onUpdate={updateTask} onShowFiles={() => { setContextOpen(true); setContextView(project?.repository ? 'source' : 'explorer'); }} /> : <TaskComposer key={`${orgId}-${initialProject}`} org={org} projects={orgProjects} agents={orgAgents} nextId={nextId} initialProject={initialProject} onCreate={createTask} onAddAgent={() => setDialog({ kind: 'agent' })} /> : <ResourceView key={`${orgId}-${section}`} section={section} projects={orgProjects} agents={orgAgents} tasks={orgTasks} automations={orgAutomations} connectors={connectors} onProject={selected => setDialog({ kind: 'project', project: selected })} onAgent={selected => setDialog({ kind: 'agent', agent: selected })} onNewTask={newTask} onConnector={connector => setConnectors(current => current.map(item => item.id === connector.id ? connector : item))} onAutomation={automation => setAutomations(current => current.map(item => item.id === automation.id ? automation : item))} onRunAutomation={automation => { const targetProject = orgProjects.find(item => item.id === automation.projectId); createTask({ id: nextId, orgId, title: automation.name, prompt: `自动化任务：${automation.name}`, projectId: automation.projectId, agentId: automation.agentId, worktree: Boolean(targetProject?.repository), cwd: taskDirectory(org, nextId, targetProject, Boolean(targetProject?.repository)), status: 'running', progress: 0, time: '刚刚', messages: [] }); }} onAdd={addResource} />}
    </main>
    {visibleContext && task && <ContextPanel key={task.id} task={task} project={project} view={contextView} onOpenFile={path => updateTask(openDocument(task, path))} onView={setContextView} onClose={() => setContextOpen(false)} />}
    {dialog?.kind === 'switcher' && <Modal title="你的组织" onClose={() => setDialog(null)}><div className="ow-org-list">{orgs.map(item => <button key={item.id} onClick={() => selectOrg(item)}><span className="ow-org-icon">{item.initials}</span><span><strong>{item.name}</strong><small>{projects.filter(project => project.orgId === item.id).length} 个项目 · {agents.filter(agent => agent.orgId === item.id).length} 位 agent</small></span>{item.id === orgId && <Check size={16} />}</button>)}<button className="ow-create-org" onClick={() => setDialog({ kind: 'org' })}><Plus size={17} />创建新组织</button></div></Modal>}
    {dialog?.kind === 'org' && <OrgForm onClose={() => setDialog(null)} onSave={created => { setOrgs(current => [...current, created]); selectOrg(created); }} />}
    {dialog?.kind === 'project' && <ProjectForm project={dialog.project} orgId={orgId} onClose={() => setDialog(null)} onSave={saved => { setProjects(current => current.some(item => item.id === saved.id) ? current.map(item => item.id === saved.id ? saved : item) : [...current, saved]); setDialog(null); }} />}
    {dialog?.kind === 'agent' && <AgentForm agent={dialog.agent} orgId={orgId} connectors={connectors} onClose={() => setDialog(null)} onSave={saved => { setAgents(current => current.some(item => item.id === saved.id) ? current.map(item => item.id === saved.id ? saved : item) : [...current, saved]); setDialog(null); }} />}
    {dialog?.kind === 'help' && <Modal title="以任务为中心的工作空间" onClose={() => setDialog(null)}><div className="ow-preview-help"><Sparkles size={25} /><p>Org → Tasks / Projects / Agents / Automation</p><p>Task 是工作入口。Project 提供可选上下文，Agent 提供角色与执行能力，worktree 和 terminal 属于任务的执行环境。</p><p>这是独立前端设计预览。数据保存在内存中，刷新后恢复；不会创建真实目录、worktree 或 agent 进程。</p><a href="?ui=desktop">返回当前 AoW<ArrowUpRight size={14} /></a></div></Modal>}
  </div>;
}
