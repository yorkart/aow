// Counterpart of Zed's session archive: history and new-agent selection live here.
import { useEffect, useState } from 'react';
import { Bot, Download, LoaderCircle, Plus, RefreshCw, Search, Trash2 } from 'lucide-react';
import { acpApi, sessionsChanged, settingsChanged } from '../api';
import { failure, type AgentInfo, type SessionInfo } from '../types';
import { ThreadImport } from './thread_import';
import { Popover } from './popover';
import '../zed.css';

// threads_archive_view.rs::format_history_entry_timestamp
function timestamp(value: string) {
  const minutes = Math.max(1, Math.floor((Date.now() - Date.parse(value)) / 60000));
  if (!Number.isFinite(minutes)) return '';
  const hours = Math.floor(minutes / 60); const days = Math.floor(hours / 24);
  return minutes < 60 ? `${minutes}m` : hours < 24 ? `${hours}h` : days < 7 ? `${days}d` : days < 28 ? `${Math.floor(days / 7)}w` : `${Math.max(1, Math.floor(days / 30))}mo`;
}
export function AgentPanel({ workspace, visible, activeSessionId, onNew, onOpen, onDeleted }: {
  workspace: string; visible: boolean; activeSessionId?: string;
  onNew: (agentId: string) => void; onOpen: (session: SessionInfo) => void; onDeleted: (id: string) => void;
}) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [local, setLocal] = useState<SessionInfo[]>([]);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [opening, setOpening] = useState('');
  const [search, setSearch] = useState('');
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    if (!visible) return;
    setLocal([]); setImporting(false);
    const controller = new AbortController(); let timer: number;
    const poll = async () => {
      try { const values = await acpApi.sessions(workspace, controller.signal); if (!controller.signal.aborted) setLocal(values); }
      catch (reason) { if (!controller.signal.aborted) setError(failure(reason)); }
      if (!controller.signal.aborted) timer = window.setTimeout(() => { void poll(); }, 1000);
    };
    const changed = () => { window.clearTimeout(timer); void poll(); };
    void poll(); window.addEventListener(sessionsChanged, changed);
    return () => { controller.abort(); window.clearTimeout(timer); window.removeEventListener(sessionsChanged, changed); };
  }, [workspace, visible, refresh]);
  useEffect(() => {
    const changed = () => setRefresh(value => value + 1);
    window.addEventListener(settingsChanged, changed);
    return () => window.removeEventListener(settingsChanged, changed);
  }, []);
  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    setBusy(true); setError('');
    void (async () => {
      const available = await acpApi.agents();
      if (cancelled) return;
      setAgents(available);
    })().catch(reason => { if (!cancelled) setError(failure(reason)); }).finally(() => { if (!cancelled) setBusy(false); });
    return () => { cancelled = true; };
  }, [visible, workspace, refresh]);
  const remove = async (session: SessionInfo) => {
    if (!window.confirm(`删除 ACP 会话“${session.title}”？支持远端删除的 Agent 也会删除远端记录。`)) return;
    setOpening(session.id); setError('');
    try { await acpApi.remove(session.id); setLocal(items => items.filter(item => item.id !== session.id)); onDeleted(session.id); }
    catch (reason) { setError(failure(reason)); } finally { setOpening(''); }
  };
  const matches = local.filter(session => `${session.title} ${session.agent_id}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())).sort((a, b) => (Date.parse(b.updated_at) || 0) - (Date.parse(a.updated_at) || 0));
  return <section className="zed-panel zed-history-panel" aria-label="ACP 会话列表">
    <header className="zed-header"><strong title={workspace}>{workspace.split('/').filter(Boolean).at(-1) || 'ACP'}</strong><span />
      <button type="button" className="zed-quiet-button" aria-label="刷新 ACP" disabled={busy} onClick={() => setRefresh(value => value + 1)}>{busy ? <LoaderCircle className="zed-spinner" size={15} /> : <RefreshCw size={15} />}</button>
      <button type="button" className="zed-quiet-button" aria-label="导入 ACP 会话" disabled={busy} onClick={() => setImporting(true)}><Download size={15} /></button>
      <Popover label="新建 ACP 会话" trigger={<Plus size={17} />}>{close => <><div className="zed-menu-heading">新建会话</div>
        {agents.filter(agent => agent.installed && agent.supported).map(agent => <button type="button" role="menuitem" key={agent.id} onClick={() => { close(); onNew(agent.id); }}><Bot size={15} /><span>{agent.name}</span></button>)}
        {!agents.some(agent => agent.installed && agent.supported) && <p className="zed-menu-heading">请先在 Settings → ACP 安装 Agent。</p>}
      </>}</Popover>
    </header>
    <div className="zed-session-search"><Search size={13} /><input aria-label="搜索 ACP 会话" placeholder="搜索会话…" value={search} onChange={event => setSearch(event.target.value)} /></div>
    {error && <p className="zed-error" role="alert">{error}</p>}
    <div className="zed-session-list">{matches.map(session => <article key={session.id} className={activeSessionId === session.id ? 'active' : ''}>
      <button type="button" className="zed-session-open" disabled={!!opening} aria-label={`打开 ACP 会话 ${session.title}`} aria-current={activeSessionId === session.id ? 'true' : undefined} onClick={() => onOpen(session)}>
        {opening === session.id || session.status === 'working' ? <LoaderCircle className="zed-spinner" size={15} /> : <Bot size={15} />}<span><strong>{session.title}</strong><small title={`${session.agent_id} · ${session.updated_at}`}>{timestamp(session.updated_at) || session.agent_id}</small></span>
      </button><button className="zed-history-delete" type="button" aria-label={`删除 ACP 会话 ${session.title}`} disabled={!!opening} onClick={() => { void remove(session); }}><Trash2 size={13} /></button>
    </article>)}{!matches.length && !busy && <p className="zed-empty-hint">{search ? '没有匹配的会话。' : '暂无会话，点击 + 开始。'}</p>}
    </div>
    {importing && <ThreadImport workspace={workspace} agents={agents} existing={local} onClose={() => setImporting(false)} />}
  </section>;
}
