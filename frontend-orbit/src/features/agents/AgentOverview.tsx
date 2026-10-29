import { useState } from 'react';
import { saveIdentity } from './api';
import { useDirty, useOperation } from './hooks';
import type { Navigate, PanelProps, Tab } from './types';
import { executorNames, providerNames } from '../connectors/types';
import { taskStatusNames, type TaskStatus } from '../projects/types';

export function AgentOverview({ org, agent, onSaved, onDirtyChange, onTab, onNavigate }: PanelProps & { onTab: (tab: Tab) => void; onNavigate: Navigate }) {
  const [fields, setFields] = useState({ name: agent.name, role: agent.role, description: agent.description });
  const baseline = JSON.stringify({ name: agent.name, role: agent.role, description: agent.description });
  const dirty = JSON.stringify(fields) !== baseline; useDirty(dirty, onDirtyChange);
  const { busy, error, perform } = useOperation();
  return <><div className="agent-cards"><section className="agent-card"><h2>Identity</h2><form className="agent-form" onSubmit={event => { event.preventDefault(); void perform(async () => onSaved(await saveIdentity(org, agent, fields))); }}>
    <label>名称<input required maxLength={100} value={fields.name} disabled={busy} onChange={event => setFields({ ...fields, name: event.target.value })} /></label>
    <label>角色<input maxLength={100} value={fields.role} disabled={busy} onChange={event => setFields({ ...fields, role: event.target.value })} /></label>
    <label>职责说明<textarea maxLength={4000} value={fields.description} disabled={busy} onChange={event => setFields({ ...fields, description: event.target.value })} /></label>
    <div className="agent-actions"><button className="orbit-primary" type="submit" disabled={busy || !dirty || !fields.name.trim()}>保存基本信息</button><button type="button" disabled={busy || dirty} onClick={() => { void perform(async () => onSaved(await saveIdentity(org, agent, { enabled: !agent.enabled }))); }}>{agent.enabled ? '停用 Agent' : '启用 Agent'}</button></div>
  </form><p className="agent-note">{agent.status_message}</p></section>
    <section className="agent-card"><h2>Harness / Runtime</h2><dl><dt>Connector</dt><dd>{agent.connector.name}</dd><dt>Provider / 执行器</dt><dd>{providerNames[agent.connector.provider]} / {executorNames[agent.connector.executor]}</dd><dt>Model</dt><dd>{agent.runtime.model ?? 'Default'}</dd><dt>最近运行</dt><dd>{agent.last_run_at ? new Date(agent.last_run_at).toLocaleString() : '尚未运行'}</dd></dl><button type="button" onClick={() => onTab('runtime')}>Configure</button></section>
    <section className="agent-card"><h2>Instructions</h2><p>AGENTS.md</p><p className="agent-note">最近修改：{new Date(agent.instructions_updated_at).toLocaleString()}</p><button type="button" onClick={() => onTab('instructions')}>Edit instructions</button></section>
    <section className="agent-card"><h2>Skills <span className="agent-note">{agent.skills.length}</span></h2><div className="agent-tags">{agent.skills.map(skill => <span key={skill.id}>{skill.name}</span>)}</div>{!agent.skills.length && <p className="agent-note">尚未启用技能。</p>}<button type="button" onClick={() => onTab('skills')}>Manage skills</button></section></div>
    {error && <p className="agent-error" role="alert">{error}</p>}<section className="agent-recent"><h2>最近任务</h2>{agent.recent_tasks.length ? <div>{agent.recent_tasks.map(task => <button className="agent-recent-task" type="button" key={task.id} onClick={() => onNavigate('projects', task.project_id, task.id)}><span>{task.identifier}</span><strong>{task.title}</strong><span>{taskStatusNames[task.status as TaskStatus]}</span><time dateTime={task.updated_at}>{new Date(task.updated_at).toLocaleString()}</time></button>)}</div> : <p className="agent-note">还没有任务。</p>}</section></>;
}
