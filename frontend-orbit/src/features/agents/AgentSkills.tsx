import { useEffect, useState } from 'react';
import { listSkills } from '../skills/api';
import type { Skill } from '../skills/types';
import { saveSkills } from './api';
import { useDirty, useOperation } from './hooks';
import type { Navigate, PanelProps } from './types';

export function AgentSkills({ org, agent, onSaved, onDirtyChange, onNavigate }: PanelProps & { onNavigate: Navigate }) {
  const [skills, setSkills] = useState<Skill[]>(); const [loadError, setLoadError] = useState(''); const [retry, setRetry] = useState(0);
  const [selected, setSelected] = useState(agent.skills.map(skill => skill.id)); const [query, setQuery] = useState(''); const [only, setOnly] = useState(false);
  const dirty = JSON.stringify([...selected].sort()) !== JSON.stringify(agent.skills.map(skill => skill.id).sort()); useDirty(dirty, onDirtyChange);
  const { busy, error, perform } = useOperation();
  useEffect(() => { const abort = new AbortController(); void listSkills(org, abort.signal).then(setSkills).catch((error: unknown) => { if (!abort.signal.aborted) setLoadError(error instanceof Error ? error.message : '无法读取技能。'); }); return () => abort.abort(); }, [org, retry]);
  const filtered = skills?.filter(skill => `${skill.name} ${skill.description}`.toLowerCase().includes(query.toLowerCase()) && (!only || selected.includes(skill.id)));
  return <><header className="agent-panel-heading"><div><h2>Skills</h2><p className="agent-note">选择此 Agent 可使用的技能，内容由组织技能库统一维护。</p></div><button type="button" onClick={() => onNavigate('skills')}>管理 Skills</button></header>
    <div className="agent-filters"><input aria-label="搜索 Agent 技能" placeholder="搜索技能…" value={query} onChange={event => setQuery(event.target.value)} /><label><input type="checkbox" checked={only} onChange={event => setOnly(event.target.checked)} />仅显示已启用</label></div>
    {loadError ? <p className="agent-error" role="alert">{loadError}<button type="button" onClick={() => { setLoadError(''); setRetry(value => value + 1); }}>重试</button></p> : !skills ? <p role="status">正在读取技能库…</p> : !skills.length ? <div className="agent-empty"><p>技能库为空，先创建或从项目导入技能。</p><button type="button" onClick={() => onNavigate('skills')}>前往 Installed skills</button></div> : !filtered?.length ? <p className="agent-note">没有匹配的技能。</p> : <div className="agent-skill-list">{filtered.map(skill => <div className="agent-skill-row" key={skill.id}><div><button type="button" className="agent-link" onClick={() => onNavigate('skills', skill.id)}>{skill.name}</button><p>{skill.description}</p><small>v{skill.version}</small></div><label><input aria-label={`启用技能 ${skill.name}`} type="checkbox" disabled={busy} checked={selected.includes(skill.id)} onChange={event => setSelected(current => event.target.checked ? [...current, skill.id] : current.filter(id => id !== skill.id))} />启用</label></div>)}</div>}
    {error && <p className="agent-error" role="alert">{error}</p>}<footer className="agent-actions"><button className="orbit-primary" type="button" aria-busy={busy} disabled={busy || !dirty || !skills} onClick={() => { void perform(async () => onSaved(await saveSkills(org, agent, selected))); }}>保存技能选择</button></footer></>;
}
