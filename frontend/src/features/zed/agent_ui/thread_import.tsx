// thread_import.rs: discover on explicit entry, select agents, then save only metadata.
import { useEffect, useRef, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import { acpApi, sessionsChanged } from '../api';
import { authenticationRequired } from '../request';
import { failure, object, text, type AgentInfo, type ConnectionInfo, type SessionImport, type SessionInfo } from '../types';
import { Authentication } from './authentication';

interface AgentHistory {
  agent: AgentInfo; connection?: ConnectionInfo; sessions: SessionImport[];
  loading?: boolean; unsupported?: boolean; error?: string; auth?: boolean; authenticating?: boolean;
}
async function discover(agent: AgentInfo, workspace: string, active: () => boolean, connection?: ConnectionInfo): Promise<AgentHistory> {
  try {
    connection ??= await acpApi.connect(agent.id, workspace);
    const capability = object(connection.capabilities.sessionCapabilities).list;
    if (capability == null || capability === false) return { agent, connection, sessions: [], unsupported: true };
    const sessions = new Map<string, SessionImport>(); const cursors = new Set<string>();
    let cursor: string | undefined;
    while (active()) {
      const page = await acpApi.remoteSessions(connection.id, cursor);
      for (const item of page.sessions) {
        const id = text(item.sessionId);
        // Zed excludes sessions without work directories. AoW imports into the current workspace.
        if (id.trim() && item.cwd === connection.cwd && !sessions.has(id)) sessions.set(id, { remote_id: id, cwd: connection.cwd, title: text(item.title) || undefined, updated_at: text(item.updatedAt) || undefined });
      }
      cursor = page.nextCursor;
      if (!cursor || cursors.has(cursor)) break;
      cursors.add(cursor);
    }
    return { agent, connection, sessions: [...sessions.values()] };
  } catch (reason) {
    return { agent, connection, sessions: [], error: failure(reason), auth: !!connection && authenticationRequired(reason) };
  }
}
export function ThreadImport({ agents, existing, workspace, onClose }: {
  agents: AgentInfo[]; existing: SessionInfo[]; workspace: string; onClose: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const active = useRef(false);
  const [rows, setRows] = useState<AgentHistory[]>([]);
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const replace = (row: AgentHistory) => { if (active.current) setRows(items => items.map(item => item.agent.id === row.agent.id ? row : item)); };
  useEffect(() => {
    const element = dialog.current!; const previous = document.activeElement;
    element.showModal(); active.current = true;
    const available = agents.filter(agent => agent.installed && agent.supported);
    setRows(available.map(agent => ({ agent, sessions: [], loading: true })));
    let cancelled = false;
    for (const agent of available) void discover(agent, workspace, () => !cancelled).then(row => {
      if (!cancelled) setRows(items => items.map(item => item.agent.id === row.agent.id ? row : item));
    });
    return () => { cancelled = true; active.current = false; element.close(); if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); };
  }, [agents, workspace]);
  const candidates = (row: AgentHistory) => row.sessions.filter(session => !existing.some(saved => saved.agent_id === row.agent.id && saved.cwd === session.cwd && saved.remote_id === session.remote_id));
  const selected = rows.filter(row => !excluded.has(row.agent.id) && !row.loading && !row.authenticating && !row.error && candidates(row).length);
  const count = selected.reduce((sum, row) => sum + candidates(row).length, 0);
  const retry = (row: AgentHistory) => {
    replace({ ...row, loading: true, error: undefined });
    void discover(row.agent, workspace, () => active.current, row.connection).then(replace);
  };
  const authenticate = (row: AgentHistory, method: string) => {
    if (!row.connection) return;
    replace({ ...row, authenticating: true, error: undefined });
    void acpApi.authenticate(row.connection.id, method).then(() => {
      if (active.current) retry({ ...row, authenticating: false, auth: false });
    }).catch(reason => replace({ ...row, authenticating: false, error: failure(reason) }));
  };
  const importSelected = async () => {
    setBusy(true); setError('');
    try {
      for (const row of selected) {
        await acpApi.importSessions(row.connection!.id, candidates(row));
        replace({ ...row, sessions: [] });
      }
      if (active.current) onClose();
    } catch (reason) { if (active.current) setError(failure(reason)); }
    finally { window.dispatchEvent(new Event(sessionsChanged)); if (active.current) setBusy(false); }
  };
  return <dialog ref={dialog} className="zed-import-dialog" aria-labelledby="zed-import-title" aria-describedby="zed-import-description" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={event => event.stopPropagation()}>
    <h2 id="zed-import-title">导入外部 Agent 会话</h2>
    <p id="zed-import-description">选择 Agent，将当前工作区中尚未加入历史的会话导入。确认后加入会话列表，打开会话时加载内容。</p>
    <div className="zed-import-agents">{rows.map(row => {
      const total = candidates(row).length;
      return <div key={row.agent.id} className="zed-import-agent">
        <label><input type="checkbox" aria-label={`导入 ${row.agent.name}`} checked={!!total && !excluded.has(row.agent.id)} disabled={busy || !!row.loading || !!row.auth || !!row.error || !total} onChange={event => { const checked = event.target.checked; setExcluded(previous => { const next = new Set(previous); if (checked) next.delete(row.agent.id); else next.add(row.agent.id); return next; }); }} /><strong>{row.agent.name}</strong>
          {row.loading ? <span role="status"><LoaderCircle className="zed-spinner" size={14} />正在查找会话…</span> : <span>{row.unsupported ? '不支持列出会话' : row.error || row.auth ? '暂不可用' : `${total} 个可导入会话`}</span>}
        </label>
        {row.error && <p className="zed-error" role="alert">{row.error}</p>}
        {row.auth && row.connection ? <Authentication connection={row.connection} pending={!!row.authenticating} onAuthenticate={method => authenticate(row, method)} /> : row.error && <button type="button" disabled={busy || row.loading} onClick={() => retry(row)}>重试 {row.agent.name}</button>}
      </div>;
    })}{!rows.length && <p>没有已安装的 Agent。请先在 Settings → ACP 安装。</p>}</div>
    {error && <p className="zed-error" role="alert">{error}</p>}
    <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button type="button" disabled={busy || !count} onClick={() => { void importSelected(); }}>{busy ? '正在导入…' : `导入 ${count} 个会话`}</button></footer>
  </dialog>;
}
