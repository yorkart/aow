import { useEffect, useState } from 'react';
import { ChevronRight, Plus, Search, Unplug } from 'lucide-react';
import { listConnectors } from './api';
import { ConnectorForm } from './ConnectorForm';
import { authNames, executorNames, providerNames, statusNames, type Connector, type ConnectorStatus } from './types';
import './connectors.css';

export function ConnectorBadge({ status }: { status: ConnectorStatus }) { return <span className={`connector-status connector-status-${status}`}><i />{statusNames[status]}</span>; }

export function ConnectorsPage({ org, onDirtyChange, initialId }: { org: string; onDirtyChange: (dirty: boolean) => void; initialId?: string }) {
  const [editing, setEditing] = useState<string | null | undefined>(initialId);
  if (editing !== undefined) return <ConnectorForm key={editing ?? 'new'} org={org} id={editing ?? undefined} onBack={() => setEditing(undefined)} onDirtyChange={onDirtyChange} />;
  return <ConnectorList org={org} onOpen={setEditing} />;
}

function ConnectorList({ org, onOpen }: { org: string; onOpen: (id: string | null) => void }) {
  const [connectors, setConnectors] = useState<Connector[]>();
  const [query, setQuery] = useState('');
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    const abort = new AbortController();
    void listConnectors(org, abort.signal).then(next => { if (!abort.signal.aborted) setConnectors(next); }).catch((error: unknown) => { if (!abort.signal.aborted) setError(error instanceof Error ? error.message : '无法读取连接。'); });
    return () => abort.abort();
  }, [org, retry]);
  const filtered = connectors?.filter(item => `${item.name} ${providerNames[item.provider] ?? item.provider} ${executorNames[item.executor] ?? item.executor} ${item.account_note} ${item.account_summary}`.toLowerCase().includes(query.toLowerCase()));
  return <section className="connectors-page"><header className="connectors-heading"><div><h1>本地 Connector</h1><p>管理 AI 执行器与账号，为 Agent 提供可用连接。</p></div><button className="orbit-primary" type="button" onClick={() => onOpen(null)}><Plus size={16} />新增 Connector</button></header>
    <label className="connectors-search"><Search size={16} /><input aria-label="搜索连接" placeholder="搜索连接名称、Provider、账号…" value={query} onChange={event => setQuery(event.target.value)} /></label>
    {error ? <p className="connectors-error" role="alert">{error}<button type="button" onClick={() => { setError(''); setRetry(value => value + 1); }}>重试</button></p>
      : !connectors ? <p role="status">正在读取连接…</p>
      : !filtered?.length ? <div className="connectors-empty"><Unplug size={28} /><h2>{query ? '没有匹配的连接' : '还没有配置连接'}</h2><p>{query ? '试试其他名称或账号。' : '添加本地登录、独立登录或 API Key 连接。'}</p></div>
      : <div className="connectors-list">{filtered.map(connector => <button className="connector-row" key={connector.id} type="button" onClick={() => onOpen(connector.id)}><span className="connector-icon"><Unplug size={21} /></span><span className="connector-row-info"><strong>{connector.name}</strong><span>{providerNames[connector.provider]} / {executorNames[connector.executor]} · {authNames[connector.auth_method]}</span>{(connector.account_summary || connector.account_note) && <span>{[connector.account_summary, connector.account_note].filter(Boolean).join(' · ')}</span>}<small>{connector.agent_count ? `${connector.agent_count} 位 Agent 正在引用` : '还没有 Agent 引用'}</small></span><ConnectorBadge status={connector.status} /><ChevronRight size={17} /></button>)}</div>}
  </section>;
}
