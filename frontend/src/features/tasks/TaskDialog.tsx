import { useEffect, useId, useRef, useState } from 'react';
import { ArrowUp, CornerDownRight, Folder, GitBranch, GitBranchPlus, LoaderCircle } from 'lucide-react';
import { AgentIcon } from '../agents/AgentIcon';
import { TaskPicker } from './TaskPicker';
import { t } from '../../lib/i18n';
import { aowApi } from '../../aow/aowApi';
import type { AowProject } from '../../aow/types';
import { acpApi } from '../acp/api';
import type { AgentInfo } from '../acp/types';
import { tasksApi, type WorkTask } from './api';

export function TaskDialog({ project, parent, onClose, onCreated }: { project: AowProject; parent?: WorkTask; onClose: () => void; onCreated: (task: WorkTask) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const backdropPointer = useRef(false);
  const [selectedProject, setSelectedProject] = useState(project);
  const [projects, setProjects] = useState<AowProject[]>([project]);
  const titleId = useId();
  const [requestId] = useState(() => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join(''));
  const [prompt, setPrompt] = useState('');
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agent, setAgent] = useState(parent?.agent_runtime_id ?? '');
  const [workspace, setWorkspace] = useState(parent?.workspace ?? selectedProject.worktrees.find(item => item.is_main)?.path ?? selectedProject.worktrees[0]?.path ?? '');
  const [isolated, setIsolated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    dialog.current?.showModal();
    let cancelled = false;
    if (!parent) void aowApi.projects().then(items => { if (!cancelled) setProjects(items.filter(item => !item.builtin)); }).catch(reason => { if (!cancelled) setError(String(reason)); });
    void acpApi.agents().then(items => { if (!cancelled) { const available = items.filter(item => item.configured && item.supported); setAgents(available); setAgent(current => current || available[0]?.id || ''); } }).catch(reason => { if (!cancelled) setError(String(reason)); });
    return () => { cancelled = true; dialog.current?.close(); };
  }, []);
  const submit = async () => {
    if (busy || !prompt.trim() || !agent) return;
    setBusy(true); setError('');
    try {
      onCreated(await tasksApi.create({ request_id: requestId, project_id: selectedProject.id, parent_id: parent?.id, prompt, agent_runtime_id: agent,
        workspace_mode: isolated ? 'new_worktree' : 'existing', ...(isolated ? {} : { workspace }) }));
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setBusy(false); }
  };
  const selectedWorktree = selectedProject.worktrees.find(item => item.path === workspace);
  const workspaceLabel = isolated ? t('tasks.new_worktree') : selectedWorktree?.is_main ? t('tasks.project_workspace') : selectedWorktree?.branch || workspace;
  const workspaceOptions = [
    { id: ':new', label: t('tasks.new_worktree'), detail: t('tasks.isolated_hint'), icon: <GitBranchPlus /> },
    ...selectedProject.worktrees.filter(item => !item.is_main).map(item => ({ id: item.path, label: item.branch || item.path.split('/').pop() || item.path, detail: item.path, group: t('tasks.reuse_worktree'), icon: <GitBranch /> })),
    ...selectedProject.worktrees.filter(item => item.is_main).map(item => ({ id: item.path, label: t('tasks.project_workspace'), detail: item.path, group: t('tasks.other_options'), icon: <Folder /> })),
    ...(workspace && !selectedWorktree ? [{ id: workspace, label: workspace.split('/').pop() || workspace, detail: workspace, icon: <GitBranch /> }] : []),
  ];
  const agentName = agents.find(item => item.id === agent)?.name ?? t('tasks.select_agent');
  return <dialog aria-labelledby={titleId} ref={dialog} className="task-dialog" onPointerDown={event => { backdropPointer.current = event.target === event.currentTarget; }}
    onClick={event => { if (backdropPointer.current && event.target === event.currentTarget && !busy) onClose(); backdropPointer.current = false; }} onCancel={event => { if (busy) event.preventDefault(); else onClose(); }}>
    <form onSubmit={event => { event.preventDefault(); void submit(); }}>
      <h2 id={titleId} className="task-composer-accessible-title">{t(parent ? 'tasks.subtask' : 'tasks.new')}</h2>
      <div className="task-composer-context">
        {parent ? <span className="task-context-project" title={selectedProject.registered_path}><Folder /><span>{selectedProject.name}</span></span> :
          <TaskPicker label={t('tasks.project')} value={selectedProject.id} options={projects.map(item => ({ id: item.id, label: item.name, detail: item.registered_path, icon: <Folder /> }))} disabled={busy} searchPlaceholder={t('tasks.search_projects')}
            onChange={id => { const next = projects.find(item => item.id === id); if (next) { setSelectedProject(next); setWorkspace(next.worktrees.find(item => item.is_main)?.path ?? next.worktrees[0]?.path ?? ''); setIsolated(false); } }}><Folder /><span>{selectedProject.name}</span></TaskPicker>}
        <TaskPicker label={t('tasks.workspace')} value={isolated ? ':new' : workspace} options={workspaceOptions} disabled={busy} searchPlaceholder={t('tasks.search_worktrees')}
          onChange={value => { setIsolated(value === ':new'); if (value !== ':new') setWorkspace(value); }}>
          {isolated ? <GitBranchPlus /> : selectedWorktree?.is_main ? <Folder /> : <GitBranch />}<span className="task-context-workspace"><span>{t('tasks.workspace')}</span><b> · </b>{workspaceLabel}</span>
        </TaskPicker>
      </div>
      <div className="task-composer-card">
        {parent && <div className="task-composer-parent" title={parent.prompt}><CornerDownRight /><span>{parent.prompt.trim().split('\n')[0]}</span></div>}
        <textarea className="task-composer-prompt" aria-label={t('tasks.prompt')} placeholder={t('tasks.describe')} autoFocus required maxLength={131072} value={prompt} disabled={busy}
          onChange={event => setPrompt(event.target.value)} onKeyDown={event => { if (event.key === 'Enter' && (event.metaKey || event.ctrlKey) && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); } }} />
        {busy && <p className="task-composer-status" role="status"><LoaderCircle className="spinning" />{t(isolated ? 'tasks.creating_workspace' : 'tasks.starting')}</p>}
        {error && <p className="task-composer-error" role="alert">{error}</p>}
        <div className="task-composer-toolbar">
          <span className="task-composer-shortcut">{t('tasks.submit_shortcut')}</span>
          <div className="task-composer-actions">
            <TaskPicker label={t('tasks.agent')} value={agent} options={agents.map(item => ({ id: item.id, label: item.name, detail: 'ACP', icon: <AgentIcon agentId={item.id} /> }))} disabled={busy} searchPlaceholder={t('tasks.search_agents')} upwards onChange={setAgent}>
              <AgentIcon agentId={agent} /><span>{agentName}</span><span className="task-composer-runtime">ACP</span>
            </TaskPicker>
            <button className="task-composer-submit" type="submit" title={t('tasks.create')} aria-label={t('tasks.create')} disabled={busy || !agent || !prompt.trim() || (!isolated && !workspace)}>
              {busy ? <LoaderCircle className="spinning" /> : <ArrowUp />}
            </button>
          </div>
        </div>
      </div>
    </form>
  </dialog>;
}
