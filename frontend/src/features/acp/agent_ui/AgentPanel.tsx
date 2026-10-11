// Counterpart of the upstream implementation's session archive: history and imports live here.
import { useEffect, useState } from 'react';
import { TaskCompletion } from '../../../components/TaskCompletion';
import { useUnreadAcpThreadIds } from '../../notifications/taskNotifications';
import { Download, LoaderCircle, RefreshCw, Search, Trash2 } from 'lucide-react';
import { acpApi, sessionsChanged, settingsChanged } from '../api';
import { failure, type AgentInfo, type SessionInfo } from '../types';
import { ThreadImport } from './thread_import';
import { AowPanelHeader } from '../../../components/AowPanelHeader';
import { AowIconButton } from '../../../components/AowIconButton';
import { useConfirmation } from '../../../components/ConfirmationDialog';
import { AcpIcon } from '../../agents/AcpIcon';
import { AgentIcon } from '../../agents/AgentIcon';
import { SpinnerLabel } from './ui/spinner_label';
import '../../../components/aow-list-row.css';
import '../acp.css';

// threads_archive_view.rs::format_history_entry_timestamp
function timestamp(value: string) {
  const minutes = Math.max(1, Math.floor((Date.now() - Date.parse(value)) / 60000));
  if (!Number.isFinite(minutes)) return '';
  const hours = Math.floor(minutes / 60); const days = Math.floor(hours / 24);
  return minutes < 60 ? `${minutes}m` : hours < 24 ? `${hours}h` : days < 7 ? `${days}d` : days < 28 ? `${Math.floor(days / 7)}w` : `${Math.max(1, Math.floor(days / 30))}mo`;
}
export function AgentPanel({ workspace, visible, activeThreadId, onOpen, onDeleted }: {
  workspace: string; visible: boolean; activeThreadId?: string;
  onOpen: (session: SessionInfo) => void; onDeleted: (id: string) => void;
}) {
  const unread = useUnreadAcpThreadIds();
  const { confirm, confirmationDialog } = useConfirmation();
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
    if (!await confirm({ title: '删除 ACP 会话？', description: `删除会话“${session.title}”。`,
      warning: '支持远端删除的 Agent 也会删除远端记录。', confirmLabel: '删除', danger: true })) return;
    setOpening(session.thread_id); setError('');
    try { await acpApi.remove(session.thread_id); setLocal(items => items.filter(item => item.thread_id !== session.thread_id)); onDeleted(session.thread_id); }
    catch (reason) { setError(failure(reason)); } finally { setOpening(''); }
  };
  const matches = local.filter(session => `${session.title} ${session.agent_id}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())).sort((a, b) => (Date.parse(b.updated_at) || 0) - (Date.parse(a.updated_at) || 0));
  return <section className="acp-history-panel" aria-label="ACP 会话列表">
    <AowPanelHeader title={workspace.split('/').filter(Boolean).at(-1) || 'ACP'} tooltip={workspace} icon={<AcpIcon />} actions={<>
      <AowIconButton title="刷新 ACP" aria-label="刷新 ACP" disabled={busy} onClick={() => setRefresh(value => value + 1)}>{busy ? <LoaderCircle className="acp-spinner" /> : <RefreshCw />}</AowIconButton>
      <AowIconButton title="导入 ACP 会话" aria-label="导入 ACP 会话" disabled={busy} onClick={() => setImporting(true)}><Download /></AowIconButton>
    </>} />
    <div className="acp-session-search"><Search size={13} /><input aria-label="搜索 ACP 会话" placeholder="搜索会话…" value={search} onChange={event => setSearch(event.target.value)} /></div>
    {error && <p className="acp-error" role="alert">{error}</p>}
    <div className="acp-session-list">{matches.map(session => <article key={session.thread_id} className={`aow-list-row${activeThreadId === session.thread_id ? ' active' : ''}`}>
      <button type="button" className="aow-list-row-open" disabled={!!opening} aria-label={`打开 ACP 会话 ${session.title}`} aria-current={activeThreadId === session.thread_id ? 'true' : undefined} onClick={() => onOpen(session)}>
        <AgentIcon agentId={session.agent_id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /><span className="aow-list-row-content"><strong className="aow-list-row-title">{session.status === 'working' && <><span role="status" aria-label="Agent 正在处理…"><SpinnerLabel /></span>{' '}</>}{session.title}</strong><small title={`${session.agent_id} · ${session.updated_at}`}>{timestamp(session.updated_at) || session.agent_id}</small></span>
        {unread.has(session.thread_id) && <TaskCompletion />}
      </button><AowIconButton className="aow-list-row-menu" title="删除会话" aria-label={`删除 ACP 会话 ${session.title}`} disabled={!!opening} onClick={() => { void remove(session); }}><Trash2 /></AowIconButton>
    </article>)}{!matches.length && !busy && <p className="acp-empty-hint">{search ? '没有匹配的会话。' : '暂无会话，请在工作区标题栏的“新建窗体”菜单中选择 ACP Agent。'}</p>}
    </div>
    {importing && <ThreadImport workspace={workspace} agents={agents} existing={local} onClose={() => setImporting(false)} />}
    {confirmationDialog}
  </section>;
}
