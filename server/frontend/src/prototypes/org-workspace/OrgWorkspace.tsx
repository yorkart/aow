import { t } from '../../lib/i18n';
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
    <nav className="ow-rail" aria-label={t('org_preview.navigation')}><button className="ow-brand" title={t('org_preview.new_task')} aria-label={t('org_preview.back_to_tasks')} onClick={() => newTask()}><Layers3 size={22} /></button><div className="ow-rail-items">{navigation.map(item => <button className={section === item.id ? 'active' : ''} aria-label={sectionLabels[item.id]} aria-current={section === item.id ? 'page' : undefined} key={item.id} onClick={() => { setSection(item.id); revealWorkspace(); }}><item.icon size={20} /><span>{sectionLabels[item.id]}</span></button>)}</div><div className="ow-rail-bottom"><button aria-label={t('org_preview.about_preview')} onClick={() => setDialog({ kind: 'help' })}><CircleHelp size={19} /></button><button className="ow-account" aria-label={t('org_preview.switch_organization')} onClick={() => setDialog({ kind: 'switcher' })}>{org.initials}</button></div></nav>
    {sidebarOpen && <aside className="ow-sidebar" aria-label={t('org_preview.section_list', { value0: sectionLabels[section] })}>
      <header className="ow-org-header"><button onClick={() => setDialog({ kind: 'switcher' })} aria-label={t('org_preview.switch_current_organization', { value0: org.name })}><span className="ow-org-icon">{org.initials}</span><strong>{org.name}</strong><ChevronDown size={14} /></button><button className="ow-icon-button" aria-label={t('org_preview.collapse_list')} onClick={() => setSidebarOpen(false)}><PanelLeftClose size={16} /></button></header>
      <div className="ow-sidebar-main-action"><button onClick={() => section === 'tasks' || section === 'automation' ? newTask() : addResource()}><Plus size={17} />{section === 'tasks' || section === 'automation' ? t('org_preview.new_task') : section === 'projects' ? t('org_preview.new_project') : t('org_preview.new_agent')}<span aria-hidden="true">＋</span></button></div>
      {section === 'tasks' ? <><label className="ow-search"><Search size={14} /><input aria-label={t('org_preview.search_tasks')} placeholder={t('org_preview.search_tasks_placeholder')} value={query} onChange={event => setQuery(event.target.value)} /></label><div className="ow-list-heading"><span>{t('org_preview.tasks')} <small>{orgTasks.length}</small></span><label><SlidersHorizontal size={12} /><select aria-label={t('org_preview.task_status_filter')} value={filter} onChange={event => setFilter(event.target.value)}><option value="all">{t('org_preview.all')}</option><option value="running">{t('org_preview.running')}</option><option value="review">{t('org_preview.awaiting_review')}</option><option value="done">{t('org_preview.completed')}</option><option value="paused">{t('org_preview.paused')}</option></select></label></div><div className="ow-task-list">{([{ title: t('org_preview.in_progress'), statuses: ['running', 'review'] }, { title: t('org_preview.recent_tasks'), statuses: ['done', 'paused'] }] as { title: string; statuses: TaskStatus[] }[]).map(group => { const entries = filteredTasks.filter(item => group.statuses.includes(item.status)); return entries.length > 0 && <div className="ow-task-group" key={group.title}><h2>{group.title}<span>{entries.length}</span></h2>{entries.map(item => { const itemProject = orgProjects.find(project => project.id === item.projectId); return <button key={item.id} className={`ow-task-row ${taskId === item.id ? 'active' : ''}`} onClick={() => { setTaskId(item.id); revealWorkspace(); }} aria-current={taskId === item.id ? 'true' : undefined}><Status status={item.status} label={false} /><span><strong>{item.title}</strong><small><span>{itemProject?.name ?? t('org_preview.no_project')}</span><span>·</span>{item.time}</small></span></button>; })}</div>; })}{!filteredTasks.length && <p className="ow-list-empty">{query || filter !== 'all' ? t('org_preview.no_matching_tasks') : t('org_preview.first_task')}</p>}</div></> : <div className="ow-resource-nav"><div className="ow-list-heading">{sectionLabels[section]}<small>{section === 'projects' ? orgProjects.length : section === 'agents' ? orgAgents.length : orgAutomations.length}</small></div>{section === 'projects' && orgProjects.map(item => <button key={item.id} onClick={() => setDialog({ kind: 'project', project: item })}><span className={`ow-project-dot ow-${item.color}`} /><span>{item.name}</span><ChevronRight size={13} /></button>)}{section === 'agents' && orgAgents.map(item => <button key={item.id} onClick={() => setDialog({ kind: 'agent', agent: item })}><Avatar agent={item} small /><span>{item.name}<small>{item.role}</small></span><ChevronRight size={13} /></button>)}{section === 'automation' && <p className="ow-help">{t('org_preview.recurring_tasks_help')}</p>}</div>}
      <footer className="ow-sidebar-footer"><span className="ow-live-dot" /><span>{runningCount ? t('org_preview.running_count', { count: runningCount }) : t('org_preview.ready_for_next_task')}</span><span className="ow-small-label">{t('org_preview.demo')}</span></footer>
    </aside>}
    <main className="ow-main"><header className="ow-main-header"><div>{!sidebarOpen && <button className="ow-icon-button" aria-label={t('org_preview.expand_list')} onClick={() => setSidebarOpen(true)}><PanelLeftOpen size={16} /></button>}<span>{sectionLabels[section]}</span><ChevronRight size={12} /><strong>{section === 'tasks' ? task ? project?.name ?? t('org_preview.standalone_task') : t('org_preview.new_task') : org.name}</strong></div><div><span className="ow-preview-badge"><span />{t('org_preview.design_preview')}</span>{task && section === 'tasks' && <button className="ow-icon-button" aria-label={contextOpen ? t('org_preview.hide_context') : t('org_preview.show_context')} aria-pressed={contextOpen} onClick={() => setContextOpen(!contextOpen)}><PanelRightOpen size={17} /></button>}</div></header>
      {section === 'tasks' ? task ? <TaskWorkspace key={task.id} task={task} agents={orgAgents} tasks={orgTasks} project={project} onSelectTask={setTaskId} onUpdate={updateTask} onShowFiles={() => { setContextOpen(true); setContextView(project?.repository ? 'source' : 'explorer'); }} /> : <TaskComposer key={`${orgId}-${initialProject}`} org={org} projects={orgProjects} agents={orgAgents} nextId={nextId} initialProject={initialProject} onCreate={createTask} onAddAgent={() => setDialog({ kind: 'agent' })} /> : <ResourceView key={`${orgId}-${section}`} section={section} projects={orgProjects} agents={orgAgents} tasks={orgTasks} automations={orgAutomations} connectors={connectors} onProject={selected => setDialog({ kind: 'project', project: selected })} onAgent={selected => setDialog({ kind: 'agent', agent: selected })} onNewTask={newTask} onConnector={connector => setConnectors(current => current.map(item => item.id === connector.id ? connector : item))} onAutomation={automation => setAutomations(current => current.map(item => item.id === automation.id ? automation : item))} onRunAutomation={automation => { const targetProject = orgProjects.find(item => item.id === automation.projectId); createTask({ id: nextId, orgId, title: automation.name, origin: 'automation', prompt: t('org_preview.automation_prompt', { value0: automation.name }), projectId: automation.projectId, agentId: automation.agentId, worktree: Boolean(targetProject?.repository), cwd: taskDirectory(org, nextId, targetProject, Boolean(targetProject?.repository)), status: 'running', progress: 0, time: t('org_preview.just_now'), messages: [] }); }} onAdd={addResource} />}
    </main>
    {visibleContext && task && <ContextPanel key={task.id} task={task} project={project} view={contextView} onOpenFile={path => updateTask(openDocument(task, path))} onView={setContextView} onClose={() => setContextOpen(false)} />}
    {dialog?.kind === 'switcher' && <Modal title={t('org_preview.your_organizations')} onClose={() => setDialog(null)}><div className="ow-org-list">{orgs.map(item => <button key={item.id} onClick={() => selectOrg(item)}><span className="ow-org-icon">{item.initials}</span><span><strong>{item.name}</strong><small>{t('org_preview.project_count', { count: projects.filter(project => project.orgId === item.id).length })} · {t('org_preview.agent_count', { count: agents.filter(agent => agent.orgId === item.id).length })}</small></span>{item.id === orgId && <Check size={16} />}</button>)}<button className="ow-create-org" onClick={() => setDialog({ kind: 'org' })}><Plus size={17} />{t('org_preview.create_new_organization')}</button></div></Modal>}
    {dialog?.kind === 'org' && <OrgForm onClose={() => setDialog(null)} onSave={created => { setOrgs(current => [...current, created]); selectOrg(created); }} />}
    {dialog?.kind === 'project' && <ProjectForm project={dialog.project} orgId={orgId} onClose={() => setDialog(null)} onSave={saved => { setProjects(current => current.some(item => item.id === saved.id) ? current.map(item => item.id === saved.id ? saved : item) : [...current, saved]); setDialog(null); }} />}
    {dialog?.kind === 'agent' && <AgentForm agent={dialog.agent} orgId={orgId} connectors={connectors} onClose={() => setDialog(null)} onSave={saved => { setAgents(current => current.some(item => item.id === saved.id) ? current.map(item => item.id === saved.id ? saved : item) : [...current, saved]); setDialog(null); }} />}
    {dialog?.kind === 'help' && <Modal title={t('org_preview.task_centered_workspace')} onClose={() => setDialog(null)}><div className="ow-preview-help"><Sparkles size={25} /><p>{t('org_preview.organization_structure')}</p><p>{t('org_preview.workspace_help')}</p><p>{t('org_preview.preview_help')}</p><a href="?ui=desktop">{t('org_preview.back_to_aow')}<ArrowUpRight size={14} /></a></div></Modal>}
  </div>;
}
