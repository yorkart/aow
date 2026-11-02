import { useEffect, useId, useRef, useState } from 'react';
import { ArrowUp, Folder, GitBranch, GitBranchPlus, LoaderCircle } from 'lucide-react';
import { AgentIcon } from '../agents/AgentIcon';
import { rolesApi, type AgentRole } from '../agent-roles/api';
import { TaskPicker } from './TaskPicker';
import { t } from '../../lib/i18n';
import { aowApi } from '../../aow/aowApi';
import type { AowProject } from '../../aow/types';
import { acpApi } from '../acp/api';
import type { AgentInfo } from '../acp/types';
import { tasksApi, type WorkTask } from './api';
import './tasks.css';

export function TaskDialog({ project, task, router = false, onClose, onSaved }: {
  project: AowProject; task?: WorkTask; router?: boolean; onClose: () => void; onSaved: (task: WorkTask) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const backdropPointer = useRef(false);
  const [selectedProject, setSelectedProject] = useState(project);
  const [projects, setProjects] = useState<AowProject[]>([project]);
  const titleId = useId();
  const [requestId] = useState(() => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join(''));
  const [prompt, setPrompt] = useState(task?.prompt ?? (router ? 'Router' : ''));
  const [roles, setRoles] = useState<AgentRole[]>([]);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agent, setAgent] = useState(task?.agent_role_id ? `role:${task.agent_role_id}` : task?.agent_id ?? '');
  const [workspace, setWorkspace] = useState(task?.workspace ?? project.worktrees.find(item => item.is_main)?.path ?? project.worktrees[0]?.path ?? '');
  const [isolated, setIsolated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    const modal = dialog.current;
    modal?.showModal();
    return () => modal?.close();
  }, []);
  useEffect(() => {
    let cancelled = false;
    if (!task) void aowApi.projects().then(items => { if (!cancelled) setProjects(items.filter(item => !item.builtin)); }).catch(reason => { if (!cancelled) setError(String(reason)); });
    void rolesApi.list().then(items => { if (!cancelled) setRoles(items); }).catch(reason => { if (!cancelled) setError(String(reason)); });
    void acpApi.agents().then(items => { if (!cancelled) { const available = items.filter(item => item.installed && item.supported); setAgents(available); setAgent(current => current || available[0]?.id || ''); } }).catch(reason => { if (!cancelled) setError(String(reason)); });
    return () => { cancelled = true; };
  }, [task]);
  const submit = async () => {
    if (busy || !prompt.trim() || (!task && (!agent || (!isolated && !workspace)))) return;
    setBusy(true); setError('');
    try {
      if (router) {
        onSaved(await tasksApi.router(project.id, agent.startsWith('role:') ? { agent_role_id: agent.slice(5) } : { agent_id: agent }));
      } else if (task) {
        await tasksApi.update(task.id, { prompt });
        onSaved({ ...task, prompt });
      } else {
        onSaved(await tasksApi.create({ request_id: requestId, project_id: selectedProject.id, prompt, agent_id: agent.startsWith('role:') ? '' : agent, agent_role_id: agent.startsWith('role:') ? agent.slice(5) : undefined, dependencies: [],
          ...(isolated ? { workspace_mode: 'new_worktree' } : { workspace_mode: 'existing', workspace }) }));
      }
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  const selectedWorktree = selectedProject.worktrees.find(item => item.path === workspace);
  const workspaceLabel = isolated ? t('tasks.new_worktree') : selectedWorktree?.is_main ? t('tasks.project_workspace') : selectedWorktree?.branch || workspace;
  const workspaceOptions = [
    { id: ':new', label: t('tasks.new_worktree'), detail: t('tasks.isolated_hint'), icon: <GitBranchPlus /> },
    ...selectedProject.worktrees.filter(item => !item.is_main).map(item => ({ id: item.path, label: item.branch || item.path.split('/').pop() || item.path, detail: item.path, group: t('tasks.reuse_worktree'), icon: <GitBranch /> })),
    ...selectedProject.worktrees.filter(item => item.is_main).map(item => ({ id: item.path, label: t('tasks.project_workspace'), detail: item.path, group: t('tasks.other_options'), icon: <Folder /> })),
    ...(workspace && !selectedWorktree ? [{ id: workspace, label: workspace.split('/').pop() || workspace, detail: workspace, icon: <GitBranch /> }] : []),
  ];
  const selectedRole = roles.find(role => `role:${role.id}` === agent);
  const runtimeLabel = selectedRole?.runtime.mode.toUpperCase() ?? 'ACP';
  const agentName = selectedRole?.name ?? agents.find(item => item.id === agent)?.name ?? (task ? agent : t('tasks.select_agent'));
  return <dialog aria-labelledby={titleId} ref={dialog} className="task-dialog" onPointerDown={event => { backdropPointer.current = event.target === event.currentTarget; }}
    onClick={event => { if (backdropPointer.current && event.target === event.currentTarget && !busy) onClose(); backdropPointer.current = false; }} onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}>
    <form onSubmit={event => { event.preventDefault(); void submit(); }}>
      <h2 id={titleId} className="task-composer-accessible-title">{t(router ? 'tasks.open_router' : task ? 'tasks.edit' : 'tasks.new')}</h2>
      <div className="task-composer-context">
        <TaskPicker label={t('tasks.project')} value={selectedProject.id} options={projects.map(item => ({ id: item.id, label: item.name, detail: item.registered_path, icon: <Folder /> }))} disabled={busy || !!task || router} searchPlaceholder={t('tasks.search_projects')}
          onChange={id => { const next = projects.find(item => item.id === id); if (next) { setSelectedProject(next); setWorkspace(next.worktrees.find(item => item.is_main)?.path ?? next.worktrees[0]?.path ?? ''); setIsolated(false); } }}><Folder /><span>{selectedProject.name}</span></TaskPicker>
        <TaskPicker label={t('tasks.workspace')} value={isolated ? ':new' : workspace} options={workspaceOptions} disabled={busy || !!task || router} searchPlaceholder={t('tasks.search_worktrees')}
          onChange={value => { setIsolated(value === ':new'); if (value !== ':new') setWorkspace(value); }}>
          {isolated ? <GitBranchPlus /> : selectedWorktree?.is_main ? <Folder /> : <GitBranch />}<span className="task-context-workspace"><span>{t('tasks.workspace')}</span><b> · </b>{workspaceLabel}</span>
        </TaskPicker>
      </div>
      <div className="task-composer-card">
        {router ? <p>{t('tasks.router_hint')}</p> : <textarea className="task-composer-prompt" aria-label={t('tasks.prompt')} placeholder={t('tasks.describe')} autoFocus required maxLength={131072} value={prompt} disabled={busy}
          onChange={event => setPrompt(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); } }} />}
        {busy && <p className="task-composer-status" role="status"><LoaderCircle className="spinning" />{t(task ? 'tasks.saving' : isolated ? 'tasks.creating_workspace' : 'tasks.starting')}</p>}
        {error && <p className="task-composer-error" role="alert">{error}</p>}
        <div className="task-composer-toolbar">
          <span className="task-composer-shortcut">{t('tasks.submit_shortcut')}</span>
          <div className="task-composer-actions">
            <TaskPicker label={t('tasks.agent')} value={agent} options={[...roles.filter(role => !router || role.runtime.mode === 'acp').map(role => ({ id: `role:${role.id}`, label: role.name, detail: role.runtime.mode.toUpperCase(), group: t('roles.title'), icon: <AgentIcon agentId={role.home_layout} /> })), ...agents.map(item => ({ id: item.id, label: item.name, detail: 'ACP', group: 'ACP', icon: <AgentIcon agentId={item.id} /> }))]} disabled={busy || !!task} searchPlaceholder={t('tasks.search_agents')} upwards onChange={setAgent}>
              <AgentIcon agentId={agent} /><span>{agentName}</span><span className="task-composer-runtime">{runtimeLabel}</span>
            </TaskPicker>
            <button className="task-composer-submit" type="submit" title={t(router ? 'tasks.open_router' : task ? 'tasks.save' : 'tasks.create')} aria-label={t(router ? 'tasks.open_router' : task ? 'tasks.save' : 'tasks.create')} disabled={busy || !prompt.trim() || (!task && (!agent || (!isolated && !workspace)))}>
              {busy ? <LoaderCircle className="spinning" /> : <ArrowUp />}
            </button>
          </div>
        </div>
      </div>
    </form>
  </dialog>;
}
