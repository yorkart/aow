import { useEffect, useId, useRef, useState } from 'react';
import { t } from '../../lib/i18n';
import { aowApi } from '../../aow/aowApi';
import type { AowProject } from '../../aow/types';
import { acpApi } from '../acp/api';
import type { AgentInfo } from '../acp/types';
import { tasksApi, type WorkTask } from './api';

export function TaskDialog({ project, parent, onClose, onCreated }: { project: AowProject; parent?: WorkTask; onClose: () => void; onCreated: (task: WorkTask) => void }) {
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const [requestId] = useState(() => Array.from(crypto.getRandomValues(new Uint8Array(16)), byte => byte.toString(16).padStart(2, '0')).join(''));
  const [prompt, setPrompt] = useState('');
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agent, setAgent] = useState(parent?.agent_id ?? '');
  const [workspace, setWorkspace] = useState(parent?.workspace ?? project.worktrees.find(item => item.is_main)?.path ?? project.worktrees[0]?.path ?? '');
  const [isolated, setIsolated] = useState(false);
  const [branch, setBranch] = useState('');
  const [path, setPath] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [jobId, setJobId] = useState<string>();
  useEffect(() => {
    dialog.current?.showModal();
    let cancelled = false;
    void acpApi.agents().then(items => { if (!cancelled) { const available = items.filter(item => item.configured && item.supported); setAgents(available); setAgent(current => current || available[0]?.id || ''); } }).catch(reason => { if (!cancelled) setError(String(reason)); });
    return () => { cancelled = true; dialog.current?.close(); };
  }, []);
  const submit = async () => {
    setBusy(true); setError('');
    try {
      if (isolated) {
        const job = await aowApi.createWorktree(project.id, { branch, path, baseRef: project.worktrees.find(item => item.is_main)?.branch || 'HEAD', pullFirst: false });
        setJobId(job.id); return;
      }
      onCreated(await tasksApi.create({ request_id: requestId, project_id: project.id, parent_id: parent?.id, prompt, agent_id: agent, workspace }));
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); setBusy(false); }
  };
  useEffect(() => {
    if (!jobId) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const job = (await aowApi.worktreeCreations()).find(item => item.id === jobId);
        if (cancelled) return;
        if (job?.status === 'succeeded') {
          setWorkspace(job.path); setIsolated(false);
          try { onCreated(await tasksApi.create({ request_id: requestId, project_id: project.id, parent_id: parent?.id, prompt, agent_id: agent, workspace: job.path })); }
          catch (reason) { if (!cancelled) { setError(String(reason)); setJobId(undefined); setBusy(false); } }
          return;
        }
        if (job && !['pending', 'running'].includes(job.status)) { setError(job.error ?? t('tasks.workspace_failed')); setJobId(undefined); setBusy(false); return; }
      } catch (reason) { if (!cancelled) setError(String(reason)); }
      if (!cancelled) timer = setTimeout(() => void poll(), 1000);
    };
    void poll();
    return () => { cancelled = true; clearTimeout(timer); };
  }, [jobId]);
  return <dialog aria-labelledby={titleId} ref={dialog} className="project-aow-modal project-aow-dialog task-dialog" onCancel={event => { if (busy) event.preventDefault(); else onClose(); }}>
    <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void submit(); }}>
      <div className="project-aow-dialog-body"><h2 id={titleId}>{t(parent ? 'tasks.subtask' : 'tasks.new')}</h2>
      <label>{t('tasks.prompt')}<textarea autoFocus required maxLength={131072} value={prompt} disabled={busy} onChange={event => setPrompt(event.target.value)} /></label>
      <label>{t('tasks.agent')}<select aria-label={t('tasks.agent')} required value={agent} disabled={busy} onChange={event => setAgent(event.target.value)}><option value="">{t('tasks.select_agent')}</option>{agents.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
      <label>{t('tasks.workspace')}<select aria-label={t('tasks.workspace')} value={isolated ? ':new' : workspace} disabled={busy} onChange={event => { setIsolated(event.target.value === ':new'); if (event.target.value !== ':new') setWorkspace(event.target.value); }}>
        {project.worktrees.map(item => <option key={item.path} value={item.path}>{item.is_main ? t('tasks.main') : item.branch} · {item.path}</option>)}
        {workspace && !project.worktrees.some(item => item.path === workspace) && <option value={workspace}>{workspace}</option>}
        <option value=":new">{t('tasks.isolated')}</option>
      </select></label>
      {isolated && <><label>{t('tasks.branch')}<input required value={branch} disabled={busy} onChange={event => setBranch(event.target.value)} /></label><label>{t('tasks.path')}<input required placeholder="/absolute/path" value={path} disabled={busy} onChange={event => setPath(event.target.value)} /></label></>}
      {jobId && <p role="status">{t('tasks.creating_workspace')}</p>}
      {error && <p className="project-aow-error" role="alert">{error}</p>}
      </div><div className="project-aow-dialog-footer"><button className="project-aow-dialog-button" type="button" disabled={busy} onClick={onClose}>{t('tasks.cancel')}</button><button className="project-aow-dialog-button primary" type="submit" disabled={busy || !agent}>{t(isolated ? 'tasks.prepare_workspace' : 'tasks.create')}</button></div>
    </form>
  </dialog>;
}
