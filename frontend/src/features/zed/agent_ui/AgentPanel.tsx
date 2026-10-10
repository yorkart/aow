// Counterpart of Zed's session archive: history and new-agent selection live here.
import { useEffect, useState } from 'react';
import { Bot, LoaderCircle, Plus, RefreshCw, Search, Trash2 } from 'lucide-react';
import { acpApi, sessionsChanged, settingsChanged } from '../api';
import { authenticationRequired } from '../request';
import { failure, object, text, type AgentInfo, type ConnectionInfo, type SessionInfo } from '../types';
import { Authentication } from './authentication';
import { Popover } from './popover';
import '../zed.css';

interface ArchivedSession extends SessionInfo { connection?: ConnectionInfo }
// threads_archive_view.rs::format_history_entry_timestamp
function timestamp(value: string) {
  const minutes = Math.max(1, Math.floor((Date.now() - Date.parse(value)) / 60000));
  if (!Number.isFinite(minutes)) return '';
  const hours = Math.floor(minutes / 60); const days = Math.floor(hours / 24);
  return minutes < 60 ? `${minutes}m` : hours < 24 ? `${hours}h` : days < 7 ? `${days}d` : days < 28 ? `${Math.floor(days / 7)}w` : `${Math.max(1, Math.floor(days / 30))}mo`;
}
const archive = (workspace: string, connection: ConnectionInfo, items: Record<string, unknown>[]) => items.filter(item => !item.cwd || item.cwd === workspace || item.cwd === connection.cwd).map(item => ({
    id: `${connection.agent_id}:${text(item.sessionId)}`, remote_id: text(item.sessionId), agent_id: connection.agent_id, cwd: workspace,
    title: text(item.title) || 'New conversation', status: 'idle', updated_at: text(item.updatedAt), connection,
  }));
export function AgentPanel({ workspace, visible, activeSessionId, onNew, onOpen, onDeleted }: {
  workspace: string; visible: boolean; activeSessionId?: string;
  onNew: (agentId: string) => void; onOpen: (session: SessionInfo) => void; onDeleted: (id: string) => void;
}) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [local, setLocal] = useState<SessionInfo[]>([]);
  const [remote, setRemote] = useState<ArchivedSession[]>([]);
  const [pages, setPages] = useState<{ connection: ConnectionInfo; cursor: string }[]>([]);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [opening, setOpening] = useState('');
  const [search, setSearch] = useState('');
  const [refresh, setRefresh] = useState(0);
  const [auth, setAuth] = useState<ConnectionInfo>();
  const [authBusy, setAuthBusy] = useState(false);
  useEffect(() => {
    if (!visible) return;
    const controller = new AbortController(); let timer: number;
    const poll = async () => {
      try { const values = await acpApi.sessions(workspace, controller.signal); if (!controller.signal.aborted) setLocal(values); }
      catch (reason) { if (!controller.signal.aborted) setError(failure(reason)); }
      if (!controller.signal.aborted) timer = window.setTimeout(() => { void poll(); }, 1000);
    };
    const changed = () => { window.clearTimeout(timer); void poll(); };
    void poll(); window.addEventListener(sessionsChanged, changed);
    return () => { controller.abort(); window.clearTimeout(timer); window.removeEventListener(sessionsChanged, changed); };
  }, [workspace, visible]);
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
      const history: ArchivedSession[] = []; const more: { connection: ConnectionInfo; cursor: string }[] = [];
      for (const agent of available.filter(item => item.installed && item.supported)) {
        if (cancelled) return;
        let connection: ConnectionInfo | undefined;
        try {
          connection = await acpApi.connect(agent.id, workspace);
          if (cancelled) return;
          const capabilities = object(connection.capabilities.sessionCapabilities);
          if (capabilities.list == null || capabilities.list === false) continue;
          const result = await acpApi.remoteSessions(connection.id);
          history.push(...archive(workspace, connection, result.sessions));
          if (result.nextCursor) more.push({ connection, cursor: result.nextCursor });
        } catch (reason) {
          if (!cancelled) { if (connection && authenticationRequired(reason)) setAuth(connection); else setError(`${agent.name}: ${failure(reason)}`); }
        }
      }
      if (!cancelled) { setRemote(history); setPages(more); }
    })().catch(reason => { if (!cancelled) setError(failure(reason)); }).finally(() => { if (!cancelled) setBusy(false); });
    return () => { cancelled = true; };
  }, [visible, workspace, refresh]);
  const open = async (session: ArchivedSession) => {
    if (!session.connection) { onOpen(session); return; }
    setOpening(session.id); setError('');
    try { const loaded = await acpApi.load(session.connection.id, session.remote_id); onOpen(loaded); window.dispatchEvent(new Event(sessionsChanged)); }
    catch (reason) { if (authenticationRequired(reason)) setAuth(session.connection); else setError(failure(reason)); }
    finally { setOpening(''); }
  };
  const remove = async (session: SessionInfo) => {
    if (!window.confirm(`删除 ACP 会话“${session.title}”？支持远端删除的 Agent 也会删除远端记录。`)) return;
    setOpening(session.id); setError('');
    try { await acpApi.remove(session.id); setLocal(items => items.filter(item => item.id !== session.id)); setRemote(items => items.filter(item => item.agent_id !== session.agent_id || item.remote_id !== session.remote_id)); onDeleted(session.id); }
    catch (reason) { setError(failure(reason)); } finally { setOpening(''); }
  };
  const sessions: ArchivedSession[] = [...local, ...remote.filter(item => !local.some(saved => saved.agent_id === item.agent_id && saved.remote_id === item.remote_id))];
  const matches = sessions.filter(session => `${session.title} ${session.agent_id}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())).sort((a, b) => (Date.parse(b.updated_at) || 0) - (Date.parse(a.updated_at) || 0));
  return <section className="zed-panel zed-history-panel" aria-label="ACP 会话列表">
    <header className="zed-header"><strong title={workspace}>{workspace.split('/').filter(Boolean).at(-1) || 'ACP'}</strong><span />
      <button type="button" className="zed-quiet-button" aria-label="刷新 ACP" disabled={busy} onClick={() => setRefresh(value => value + 1)}>{busy ? <LoaderCircle className="zed-spinner" size={15} /> : <RefreshCw size={15} />}</button>
      <Popover label="新建 ACP 会话" trigger={<Plus size={17} />}>{close => <><div className="zed-menu-heading">新建会话</div>
        {agents.filter(agent => agent.installed && agent.supported).map(agent => <button type="button" role="menuitem" key={agent.id} onClick={() => { close(); onNew(agent.id); }}><Bot size={15} /><span>{agent.name}</span></button>)}
        {!agents.some(agent => agent.installed && agent.supported) && <p className="zed-menu-heading">请先在 Settings → ACP 安装 Agent。</p>}
      </>}</Popover>
    </header>
    <div className="zed-session-search"><Search size={13} /><input aria-label="搜索 ACP 会话" placeholder="搜索会话…" value={search} onChange={event => setSearch(event.target.value)} /></div>
    {error && <p className="zed-error" role="alert">{error}</p>}
    {auth && <Authentication connection={auth} pending={authBusy} onAuthenticate={method => { setAuthBusy(true); void acpApi.authenticate(auth.id, method).then(() => { setAuth(undefined); setRefresh(value => value + 1); }).catch(reason => setError(failure(reason))).finally(() => setAuthBusy(false)); }} />}
    <div className="zed-session-list">{matches.map(session => <article key={session.id} className={activeSessionId === session.id ? 'active' : ''}>
      <button type="button" className="zed-session-open" disabled={!!opening} aria-label={`打开 ACP 会话 ${session.title}`} aria-current={activeSessionId === session.id ? 'true' : undefined} onClick={() => { void open(session); }}>
        {opening === session.id || session.status === 'working' ? <LoaderCircle className="zed-spinner" size={15} /> : <Bot size={15} />}<span><strong>{session.title}</strong><small title={`${session.agent_id} · ${session.updated_at}`}>{timestamp(session.updated_at) || session.agent_id}</small></span>
      </button>{!session.connection && <button className="zed-history-delete" type="button" aria-label={`删除 ACP 会话 ${session.title}`} disabled={!!opening} onClick={() => { void remove(session); }}><Trash2 size={13} /></button>}
    </article>)}{!matches.length && !busy && <p className="zed-empty-hint">{search ? '没有匹配的会话。' : '暂无会话，点击 + 开始。'}</p>}
      {!!pages.length && <button type="button" className="zed-load-more" disabled={busy} onClick={() => { setBusy(true); setError(''); void (async () => {
        const nextPages = [];
        for (const page of pages) { const result = await acpApi.remoteSessions(page.connection.id, page.cursor); setRemote(items => [...items, ...archive(workspace, page.connection, result.sessions).filter(item => !items.some(previous => previous.id === item.id))]); if (result.nextCursor) nextPages.push({ connection: page.connection, cursor: result.nextCursor }); }
        setPages(nextPages);
      })().catch(reason => setError(failure(reason))).finally(() => setBusy(false)); }}>加载更多</button>}
    </div>
  </section>;
}
