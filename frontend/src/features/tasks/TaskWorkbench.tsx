import { useCallback, useEffect, useRef, useState, type CSSProperties, type ReactNode, type PointerEvent as ReactPointerEvent } from 'react';
import { Archive, Files, ListTodo, Pin, PinOff, Plus, RefreshCw, SquareTerminal, Trash2 } from 'lucide-react';
import { t } from '../../lib/i18n';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowIconButton } from '../../components/AowIconButton';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { aowApi } from '../../aow/aowApi';
import { ProjectIcon } from '../../aow/ProjectIcon';
import { AowNodeSwitcher } from '../../aow/AowNodeSwitcher';
import type { AowProject, AowWorktree } from '../../aow/types';
import type { AowAgent } from '../agents/types';
import { agentsApi } from '../agents/api';
import { SystemFileBrowser } from '../files/SystemFileBrowser';
import { tasksApi, type WorkTask } from './api';
import { TaskDialog } from './TaskDialog';
import './tasks.css';

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);
interface SidebarControls { visible: boolean; hide: () => void; show: () => void; resize: (event: ReactPointerEvent<HTMLDivElement>) => void }
type WorkspaceRenderer = (task: WorkTask, project: AowProject, worktree: AowWorktree, agents: AowAgent[], active: boolean, onCreateTask: () => void, sidebar: SidebarControls) => ReactNode;
export function TaskWorkbench({ visible, onExit, renderWorkspace }: { visible: boolean; onExit: () => void; renderWorkspace: WorkspaceRenderer }) {
  const [projects, setProjects] = useState<AowProject[]>([]);
  const [agents, setAgents] = useState<AowAgent[]>([]);
  const [tasks, setTasks] = useState<WorkTask[]>([]);
  const [activeId, setActiveId] = useState<string>();
  const [visited, setVisited] = useState(new Set<string>());
  const [dialog, setDialog] = useState<{ project: AowProject; parent?: WorkTask }>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [browser, setBrowser] = useState(false);
  const [leftWidth, setLeftWidth] = useState(280);
  const [rightWidth, setRightWidth] = useState(340);
  const [rightVisible, setRightVisible] = useState(true);
  const resize = (side: 'left' | 'right', event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    const element = event.currentTarget;
    element.setPointerCapture(event.pointerId);
    const start = event.clientX, width = side === 'left' ? leftWidth : rightWidth;
    const move = (next: PointerEvent) => {
      const value = Math.max(side === 'left' ? 180 : 220, Math.min(side === 'left' ? 520 : 640, width + (next.clientX - start) * (side === 'left' ? 1 : -1)));
      if (side === 'left') setLeftWidth(value); else setRightWidth(value);
    };
    const end = () => { element.removeEventListener('pointermove', move); element.removeEventListener('lostpointercapture', end); };
    element.addEventListener('pointermove', move); element.addEventListener('lostpointercapture', end);
  };
  const [archives, setArchives] = useState(false);
  const { confirm, confirmationDialog } = useConfirmation();
  const request = useRef(0);
  const refresh = useCallback(async () => {
    const version = ++request.current;
    try {
      const [nextProjects, nextTasks, nextAgents] = await Promise.all([aowApi.projects(), tasksApi.list(), agentsApi.agents(true)]);
      if (version !== request.current) return;
      setProjects(nextProjects.filter(project => !project.builtin)); setTasks(nextTasks); setAgents(nextAgents); setError('');
    } catch (reason) { if (version === request.current) setError(failure(reason)); }
  }, []);
  useEffect(() => { if (visible) void refresh(); }, [visible, refresh]);
  useEffect(() => {
    if (!visible) return;
    const timer = window.setInterval(() => { if (document.visibilityState === 'visible') void refresh(); }, 15000);
    return () => clearInterval(timer);
  }, [visible, refresh]);
  const select = (task: WorkTask) => { if (task.archived) return; setActiveId(task.id); setVisited(current => new Set(current).add(task.id)); };
  const mutate = async (action: () => Promise<unknown>) => {
    setBusy(true); setError(''); ++request.current;
    try { await action(); await refresh(); } catch (reason) { setError(failure(reason)); } finally { setBusy(false); }
  };
  const finish = async (task: WorkTask, archive: boolean) => {
    if (!await confirm({ title: archive ? t('tasks.archive') : t('tasks.delete'), description: t('tasks.cleanup_description'), danger: !archive })) return;
    await mutate(() => tasksApi.finish(task, tasks, archive));
  };
  const row = (task: WorkTask) => <div className="task-tree-row">
    <button className={activeId === task.id ? 'active' : ''} onClick={() => select(task)} disabled={task.archived} title={task.prompt}><ListTodo /><span>{task.prompt.split('\n')[0]}</span></button>
    {!task.archived && <><AowIconButton title={t(task.pinned ? 'tasks.unpin' : 'tasks.pin')} disabled={busy} onClick={() => void mutate(() => tasksApi.pin(task.id, !task.pinned))}>{task.pinned ? <PinOff /> : <Pin />}</AowIconButton>
      <AowIconButton title={t('tasks.subtask')} disabled={busy} onClick={() => { const project = projects.find(item => item.id === task.project_id); if (project) setDialog({ project, parent: task }); }}><Plus /></AowIconButton>
      <AowIconButton title={t('tasks.archive')} disabled={busy} onClick={() => void finish(task, true)}><Archive /></AowIconButton></>}
    <AowIconButton title={t('tasks.delete')} disabled={busy} onClick={() => void finish(task, false)}><Trash2 /></AowIconButton>
  </div>;
  const tree = (parent: string | null, project: AowProject): ReactNode => <ul role="group" className="task-tree">{tasks.filter(task => task.project_id === project.id && task.parent_id === parent && task.archived === archives).map(task => <li key={task.id}>{row(task)}{tree(task.id, project)}</li>)}</ul>;
  const active = tasks.find(task => task.id === activeId && !task.archived);
  return <div className={`project-aow-layout task-workbench${rightVisible ? '' : ' right-sidebar-hidden'}`} style={{ '--project-left-width': `${leftWidth}px`, '--project-right-width': `${rightWidth}px` } as CSSProperties}>
    <aside className="project-aow-projects"><div className="project-aow-project-panel">
      <header className="project-aow-brand"><AowNodeSwitcher /><strong>{t('tasks.workbench')}</strong></header>
      <AowPanelStack className="project-aow-project-list" dockHeaders={false}>
        <AowPanel title={t('navigation.pinned')} collapsible={false}>{tasks.filter(task => task.pinned && !task.archived).map(task => <div key={task.id}>{row(task)}</div>)}</AowPanel>
        <header className="side-title aow-panel-header"><strong>{t('navigation.projects')}</strong><span className="aow-panel-header-actions"><AowIconButton title={t('tasks.refresh')} onClick={() => void refresh()}><RefreshCw /></AowIconButton><AowIconButton title={t('tasks.archived')} onClick={() => setArchives(value => !value)}><Archive /></AowIconButton></span></header>
        {projects.map(project => <AowPanel key={project.id} title={project.name} icon={<ProjectIcon project={project} />} actions={<AowIconButton title={t('tasks.new')} onClick={() => setDialog({ project })}><Plus /></AowIconButton>}>{tree(null, project)}</AowPanel>)}
      </AowPanelStack>
    </div><div className="project-aow-sidebar-footer"><button title={t('aow.open_system_file_browser')} onClick={() => setBrowser(true)}><Files /></button><button title={t('tasks.exit')} onClick={onExit}><ListTodo /></button></div></aside>
    <div className="project-aow-left-resizer" role="separator" aria-label={t('aow.resize_projects_sidebar')} aria-orientation="vertical" onPointerDown={event => resize('left', event)} />
    <div className="project-aow-workspace"><div className="project-aow-surfaces">
      {error && <div className="project-aow-inline-error" role="alert">{error}</div>}
      {tasks.filter(task => !task.archived && visited.has(task.id)).map(task => {
        const project = projects.find(item => item.id === task.project_id);
        const worktree = project?.worktrees.find(item => item.path === task.workspace);
        return project && worktree ? <div key={task.id} className="task-workspace-host" hidden={activeId !== task.id}>{renderWorkspace(task, project, worktree, agents, visible && activeId === task.id, () => setDialog({ project, parent: task }), { visible: rightVisible, hide: () => setRightVisible(false), show: () => setRightVisible(true), resize: event => resize('right', event) })}</div> : null;
      })}
      {!active && <div className="project-aow-no-context"><ListTodo /><h1>{t('tasks.workbench')}</h1><p>{t('tasks.empty')}</p></div>}
    </div><footer className="project-aow-status"><span>{active?.workspace}</span><div className="project-aow-status-actions">{active && <button title={t('navigation.terminal')} onClick={() => window.dispatchEvent(new CustomEvent('aow:task-terminal', { detail: active.id }))}><SquareTerminal /></button>}</div></footer></div>
    {dialog && <TaskDialog key={`${dialog.project.id}:${dialog.parent?.id ?? ''}`} project={dialog.project} parent={dialog.parent} onClose={() => setDialog(undefined)} onCreated={task => { ++request.current; setTasks(current => [...current.filter(item => item.id !== task.id), task]); select(task); setDialog(undefined); void refresh(); }} />}
    {browser && <div className="task-browser"><SystemFileBrowser open onHide={() => setBrowser(false)} onOpenFile={entry => { if (active) { window.dispatchEvent(new CustomEvent('aow:task-file', { detail: { taskId: active.id, entry } })); setBrowser(false); } }} /></div>}
    {confirmationDialog}
  </div>;
}
