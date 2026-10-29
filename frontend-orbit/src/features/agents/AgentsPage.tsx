import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Plus, RefreshCw, Users } from 'lucide-react';
import { listConnectors } from '../connectors/api';
import { executorNames, type Connector } from '../connectors/types';
import { createAgent, getAgent, listAgents } from './api';
import { AgentOverview } from './AgentOverview';
import { AgentInstructions } from './AgentInstructions';
import { AgentSkills } from './AgentSkills';
import { AgentRuntime } from './AgentRuntime';
import { AgentVariables } from './AgentVariables';
import { useDirty, useOperation } from './hooks';
import { statusNames, tabs, type Agent, type Navigate, type Tab } from './types';
import './agents.css';

export function AgentsPage({ org, sidebar, onDirtyChange, onNavigate }: { org: string; sidebar: HTMLElement; onDirtyChange: (dirty: boolean) => void; onNavigate: Navigate }) {
  const [agents, setAgents] = useState<Agent[]>(); const [selected, setSelected] = useState<string | null>(null); const [query, setQuery] = useState(''); const [error, setError] = useState(''); const [retry, setRetry] = useState(0);
  const dirty = useRef(false);
  const changed = useCallback((value: boolean) => { dirty.current = value; onDirtyChange(value); }, [onDirtyChange]);
  const guard = () => !dirty.current || window.confirm('放弃尚未保存的 Agent 配置？');
  const navigate: Navigate = (section, id, taskId) => { if (guard()) { changed(false); onNavigate(section, id, taskId); } };
  const select = (id: string) => { if (id !== selected && guard()) { changed(false); setSelected(id); } };
  useEffect(() => {
    const abort = new AbortController();
    void listAgents(org, abort.signal).then(rows => { if (!abort.signal.aborted) { setAgents(rows); setSelected(current => current ?? rows[0]?.id ?? null); } }).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取 Agent。'); });
    return () => abort.abort();
  }, [org, retry]);
  const update = useCallback((agent: Agent) => { setAgents(current => current?.some(row => row.id === agent.id) ? current.map(row => row.id === agent.id ? agent : row) : [...(current ?? []), agent]); setSelected(agent.id); }, []);
  const filtered = agents?.filter(agent => `${agent.name} ${agent.role}`.toLowerCase().includes(query.toLowerCase()));
  return <>{createPortal(<div className="agents-sidebar"><header><h2>AGENTS</h2><button type="button" aria-label="新建 Agent" title="New Agent" onClick={() => select('new')}><Plus size={16} /></button></header><input aria-label="搜索 Agent" placeholder="搜索 Agent…" value={query} onChange={event => setQuery(event.target.value)} />
    {error ? <p className="agent-error" role="alert">{error}<button type="button" onClick={() => { setError(''); setRetry(value => value + 1); }}>重试</button></p> : !agents ? <p role="status">正在读取…</p> : <div className="agents-sidebar-list">{filtered?.map(agent => <button className="agent-sidebar-row" key={agent.id} type="button" aria-current={selected === agent.id ? 'page' : undefined} onClick={() => select(agent.id)}><span className="agent-avatar">{agent.name.slice(0, 1).toUpperCase()}</span><span className="agent-sidebar-name"><strong>{agent.name}</strong><small>{agent.role || executorNames[agent.connector.executor]}</small></span><span className={`agent-state agent-state-${agent.status}`}>{statusNames[agent.status]}</span></button>)}{!filtered?.length && <p className="agent-note">{query ? '没有匹配的 Agent。' : '还没有 Agent。'}</p>}</div>}
  </div>, sidebar)}<section className="agents-page">{selected === 'new' ? <NewAgent key="new" org={org} onCreated={update} onDirtyChange={changed} onNavigate={navigate} /> : selected ? <AgentDetail key={selected} org={org} id={selected} onUpdated={update} onDirtyChange={changed} onNavigate={navigate} canLeave={guard} /> : <div className="agent-empty"><Users size={30} /><h1>创建你的第一个 Agent</h1><p>为角色选择 Connector，再配置独立的指令和 Skills。</p><button className="orbit-primary" type="button" onClick={() => select('new')}>新建 Agent</button></div>}</section></>;
}

function NewAgent({ org, onCreated, onDirtyChange, onNavigate }: { org: string; onCreated: (agent: Agent) => void; onDirtyChange: (dirty: boolean) => void; onNavigate: Navigate }) {
  const [connectors, setConnectors] = useState<Connector[]>(); const [loadError, setLoadError] = useState(''); const [retry, setRetry] = useState(0);
  const [fields, setFields] = useState({ name: '', role: '', description: '', connector_id: '' });
  useDirty(Boolean(fields.name || fields.role || fields.description), onDirtyChange); const { busy, error, perform } = useOperation();
  useEffect(() => { const abort = new AbortController(); void listConnectors(org, abort.signal).then(rows => { if (!abort.signal.aborted) { const available = rows.filter(row => row.status === 'available'); setConnectors(available); setFields(current => ({ ...current, connector_id: current.connector_id || available[0]?.id || '' })); } }).catch((error: unknown) => { if (!abort.signal.aborted) setLoadError(error instanceof Error ? error.message : '无法读取连接。'); }); return () => abort.abort(); }, [org, retry]);
  return <><header className="agent-heading"><div><h1>新建 Agent</h1><p className="agent-note">创建组织内的独立执行角色。</p></div></header>
    {loadError && <p className="agent-error" role="alert">{loadError}<button type="button" onClick={() => { setLoadError(''); setRetry(value => value + 1); }}>重试</button></p>}
    {connectors && !connectors.length ? <div className="agent-empty"><h2>还没有可用的 Connector</h2><p>先创建并验证一个连接，再为 Agent 选择身份。</p><button type="button" onClick={() => onNavigate('connectors')}>前往 Connectors</button></div> : <form className="agent-form agent-new-form" onSubmit={event => { event.preventDefault(); void perform(async () => { const agent = await createAgent(org, fields); onDirtyChange(false); onCreated(agent); }); }}>
      <label>名称<input autoFocus required maxLength={100} disabled={busy} value={fields.name} placeholder="例如：Reviewer" onChange={event => setFields({ ...fields, name: event.target.value })} /></label><label>角色<input maxLength={100} disabled={busy} value={fields.role} placeholder="例如：代码审查者" onChange={event => setFields({ ...fields, role: event.target.value })} /></label><label>职责说明<textarea maxLength={4000} disabled={busy} value={fields.description} onChange={event => setFields({ ...fields, description: event.target.value })} /></label>
      <label>Connector<select aria-label="Agent Connector" required disabled={busy || !connectors} value={fields.connector_id} onChange={event => setFields({ ...fields, connector_id: event.target.value })}>{!fields.connector_id && <option value="">请选择可用连接</option>}{connectors?.map(connector => <option key={connector.id} value={connector.id}>{connector.name} · {executorNames[connector.executor]}</option>)}</select></label><p className="agent-note">{connectors?.find(row => row.id === fields.connector_id)?.account_summary}</p>{error && <p className="agent-error" role="alert">{error}</p>}<footer className="agent-actions"><button className="orbit-primary" type="submit" disabled={busy || !fields.name.trim() || !fields.connector_id}>创建 Agent</button></footer>
    </form>}</>;
}

function AgentDetail({ org, id, onUpdated, onDirtyChange, onNavigate, canLeave }: { org: string; id: string; onUpdated: (agent: Agent) => void; onDirtyChange: (dirty: boolean) => void; onNavigate: Navigate; canLeave: () => boolean }) {
  const [agent, setAgent] = useState<Agent>(); const [tab, setTab] = useState<Tab>('overview'); const [error, setError] = useState(''); const [retry, setRetry] = useState(0);
  useEffect(() => { const abort = new AbortController(); void getAgent(org, id, abort.signal).then(row => { if (!abort.signal.aborted) { setAgent(row); onUpdated(row); } }).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取 Agent。'); }); return () => abort.abort(); }, [org, id, retry, onUpdated]);
  if (!agent) return <p role={error ? 'alert' : 'status'}>{error || '正在读取 Agent…'}{error && <button type="button" onClick={() => { setError(''); setRetry(value => value + 1); }}>重试</button>}</p>;
  const changeTab = (next: Tab) => { if (next !== tab && canLeave()) { onDirtyChange(false); setTab(next); } };
  const saved = (row: Agent) => { onDirtyChange(false); setAgent(row); onUpdated(row); };
  const props = { org, agent, onSaved: saved, onDirtyChange };
  const revision = tab === 'overview' ? agent.revisions.identity : tab === 'instructions' ? agent.instructions_version_id : agent.revisions[tab];
  return <><header className="agent-heading"><div><h1>{agent.name}</h1><p>{executorNames[agent.connector.executor]}{agent.role ? ` · ${agent.role}` : ''}</p></div><div className="agent-heading-actions"><span className={`agent-state agent-state-${agent.status}`}>{statusNames[agent.status]}</span><button type="button" aria-label="刷新 Agent" onClick={() => { if (canLeave()) { onDirtyChange(false); setAgent(undefined); setRetry(value => value + 1); } }}><RefreshCw size={15} /></button></div></header>
    {agent.connector.status !== 'available' && <div className="agent-error" role="alert"><span>{agent.status_message}</span><button type="button" onClick={() => onNavigate('connectors', agent.connector.id)}>前往 Connector 配置</button></div>}
    <div className="agent-tabs" role="tablist" aria-label="Agent 配置">{tabs.map(item => <button type="button" role="tab" key={item.id} id={`agent-tab-${item.id}`} aria-controls="agent-panel" aria-selected={tab === item.id} onClick={() => changeTab(item.id)}>{item.label}</button>)}</div>
    <div className="agent-panel" role="tabpanel" id="agent-panel" aria-labelledby={`agent-tab-${tab}`} key={`${tab}:${revision}`}>{tab === 'overview' ? <AgentOverview {...props} onTab={changeTab} onNavigate={onNavigate} /> : tab === 'instructions' ? <AgentInstructions {...props} /> : tab === 'skills' ? <AgentSkills {...props} onNavigate={onNavigate} /> : tab === 'runtime' ? <AgentRuntime {...props} onNavigate={onNavigate} /> : <AgentVariables {...props} />}</div></>;
}
