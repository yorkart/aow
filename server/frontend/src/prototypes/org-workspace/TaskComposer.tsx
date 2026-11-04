import { t } from '../../lib/i18n';
import { useState } from 'react';
import { ArrowUp, Bot, Folder, FolderGit2, GitBranch, Sparkles } from 'lucide-react';
import { Avatar } from './ui';
import { taskDirectory, type Agent, type Org, type Project, type Task } from './model';

export function TaskComposer({ org, projects, agents, nextId, initialProject, onCreate, onAddAgent }: {
  org: Org; projects: Project[]; agents: Agent[]; nextId: string; initialProject: string;
  onCreate: (task: Task) => void; onAddAgent: () => void;
}) {
  const [prompt, setPrompt] = useState('');
  const [projectId, setProjectId] = useState(initialProject);
  const [agentId, setAgentId] = useState('');
  const [worktree, setWorktree] = useState(true);
  const project = projects.find(item => item.id === projectId);
  const agent = agents.find(item => item.id === agentId);
  const cwd = taskDirectory(org, nextId, project, worktree);
  const canCreate = Boolean(prompt.trim() && agent);
  return <div className="ow-new-task">
    <div className="ow-new-task-heading"><span className="ow-star"><Sparkles size={25} /></span><p>{org.name} / {t('org_preview.new_task')}</p><h1>{t('org_preview.next_task_heading')}</h1><span>{t('org_preview.next_task_description')}</span></div>
    <form className="ow-composer" onSubmit={event => {
      event.preventDefault();
      if (!canCreate) return;
      onCreate({ id: nextId, orgId: org.id, title: prompt.trim().split('\n')[0].slice(0, 55), prompt: prompt.trim(), projectId, agentId, worktree: Boolean(project?.repository && worktree), cwd, status: 'running', progress: 0, time: t('org_preview.just_now'), messages: [] });
    }}>
      <label className="ow-sr-only" htmlFor="ow-task-prompt">{t('org_preview.task_description')}</label>
      <textarea id="ow-task-prompt" value={prompt} onChange={event => setPrompt(event.target.value)} placeholder={t('org_preview.task_description_placeholder')} autoFocus required />
      <div className="ow-composer-options">
        <label><Folder size={15} /><select aria-label={t('org_preview.linked_project')} value={projectId} onChange={event => setProjectId(event.target.value)}><option value="">{t('org_preview.without_project')}</option>{projects.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
        <label>{agent ? <Avatar agent={agent} small /> : <Bot size={15} />}<select aria-label={t('org_preview.assigned_agent')} value={agentId} required onChange={event => setAgentId(event.target.value)}><option value="" disabled>{t('org_preview.select_required_agent')}</option>{agents.map(item => <option key={item.id} value={item.id}>{item.name} · {item.role}</option>)}</select></label>
        <button className="ow-send" type="submit" disabled={!canCreate} aria-label={t('org_preview.create_and_run_task')} title={t('org_preview.create_and_run_task')}><ArrowUp size={19} /></button>
      </div>
      {project?.repository && <fieldset className="ow-worktree-choice"><legend>{t('org_preview.execution_environment')}</legend>
        <label><input type="radio" name="workspace" checked={worktree} onChange={() => setWorktree(true)} /><GitBranch size={15} /><span>{t('org_preview.create_worktree')}<small>{t('org_preview.worktree_help')}</small></span></label>
        <label><input type="radio" name="workspace" checked={!worktree} onChange={() => setWorktree(false)} /><FolderGit2 size={15} /><span>{t('org_preview.use_project_repository')}<small>{t('org_preview.repository_help')}</small></span></label>
      </fieldset>}
    </form>
    <div className="ow-cwd-preview"><Folder size={14} /><span>{t('org_preview.working_directory')}<code>{cwd}</code></span><span className="ow-small-label">{project?.repository ? worktree ? t('org_preview.separate_worktree') : t('org_preview.project_repository') : t('org_preview.automatically_allocated')}</span></div>
    {!agents.length && <p className="ow-help">{t('org_preview.organization_has_no_agents')}<button className="ow-text-button" onClick={onAddAgent}>{t('org_preview.configure_first_agent')}</button></p>}
    <div className="ow-suggestions"><span>{t('org_preview.suggestions_heading')}</span>{[t('org_preview.suggest_project_review'), t('org_preview.suggest_task_flow'), t('org_preview.suggest_notes')].map(text => <button key={text} onClick={() => setPrompt(text)}>{text}<ArrowUp size={13} /></button>)}</div>
    <p className="ow-demo-note">{t('org_preview.preview_data_notice')}</p>
  </div>;
}
