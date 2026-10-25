import { t } from '../../../lib/i18n';
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
        // Upstream excludes sessions without work directories. AoW imports into the current workspace.
        if (id.trim() && item.cwd === connection.cwd && !sessions.has(id)) sessions.set(id, { session_id: id, cwd: connection.cwd, title: text(item.title) || undefined, updated_at: text(item.updatedAt) || undefined });
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
  const candidates = (row: AgentHistory) => row.sessions.filter(session => !existing.some(saved => saved.agent_id === row.agent.id && saved.cwd === session.cwd && saved.session_id === session.session_id));
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
  return <dialog ref={dialog} className="acp-import-dialog" aria-labelledby="acp-import-title" aria-describedby="acp-import-description" onCancel={event => { event.preventDefault(); if (!busy) onClose(); }} onKeyDown={event => event.stopPropagation()}>
    <h2 id="acp-import-title">{t("acp.import_external_agent_sessions")}</h2>
    <p id="acp-import-description">{t("acp.choose_an_agent_and_import_sessions_from_this_workspace")}</p>
    <div className="acp-import-agents">{rows.map(row => {
      const total = candidates(row).length;
      return <div key={row.agent.id} className="acp-import-agent">
        <label><input type="checkbox" aria-label={t("acp.import", { value0: row.agent.name })} checked={!!total && !excluded.has(row.agent.id)} disabled={busy || !!row.loading || !!row.auth || !!row.error || !total} onChange={event => { const checked = event.target.checked; setExcluded(previous => { const next = new Set(previous); if (checked) next.delete(row.agent.id); else next.add(row.agent.id); return next; }); }} /><strong>{row.agent.name}</strong>
          {row.loading ? <span role="status"><LoaderCircle className="acp-spinner" size={14} />{t("acp.finding_sessions")}</span> : <span>{row.unsupported ? t("acp.session_listing_unsupported") : row.error || row.auth ? t("acp.unavailable") : t("acp.sessions_available_to_import", { count: total, value0: total })}</span>}
        </label>
        {row.error && <p className="acp-error" role="alert">{row.error}</p>}
        {row.auth && row.connection ? <Authentication connection={row.connection} pending={!!row.authenticating} onAuthenticate={method => authenticate(row, method)} /> : row.error && <button type="button" disabled={busy || row.loading} onClick={() => retry(row)}>{t("aow.retry")} {row.agent.name}</button>}
      </div>;
    })}{!rows.length && <p>{t("acp.no_agents_installed_install_one_in_settings_acp_first")}</p>}</div>
    {error && <p className="acp-error" role="alert">{error}</p>}
    <footer><button type="button" disabled={busy} onClick={onClose}>{t("aow.cancel")}</button><button type="button" disabled={busy || !count} onClick={() => { void importSelected(); }}>{busy ? t("acp.importing") : t("acp.import_sessions", { count: count, value0: count })}</button></footer>
  </dialog>;
}
