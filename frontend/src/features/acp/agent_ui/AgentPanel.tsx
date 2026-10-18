// Counterpart of the upstream implementation's session archive: history and imports live here.
import { useEffect, useMemo, useState } from 'react';
import { useInstanceGraph } from '../../instances/useInstanceGraph';
import { InstanceDescendants, indexTerminalChildren } from '../../terminals/TerminalDescendants';
import { TaskCompletion } from '../../../components/TaskCompletion';
import { useUnreadAcpThreadIds } from '../../notifications/taskNotifications';
import { Download, Search, Trash2, UserRound, SquareTerminal } from 'lucide-react';
import { acpApi, settingsChanged } from '../api';
import { useProjectSessions } from '../sync/projectSessions';
import { compareSessionActivity, sessionDisplayTime } from '../session_activity';
import { useProjectAcpPanel } from './projectPanel';
import { useAowTabNavigation } from '../../../aow/tabNavigation';
import { InstancePanelGroups } from '../../instances/InstancePanelGroups';
import { AowListRow } from '../../../components/AowListRow';
import { failure, type AgentInfo, type SessionInfo } from '../types';
import { ThreadImport } from './thread_import';
import { removeAcpSessionTabs } from './workspace_tabs';
import { AowIconButton } from '../../../components/AowIconButton';
import { useConfirmation } from '../../../components/ConfirmationDialog';
import { AgentIcon } from '../../agents/AgentIcon';
import { SpinnerLabel } from './ui/spinner_label';
import '../acp.css';

// threads_archive_view.rs::format_history_entry_timestamp
function timestamp(value: string) {
  const minutes = Math.max(1, Math.floor((Date.now() - Date.parse(value)) / 60000));
  if (!Number.isFinite(minutes)) return '';
  const hours = Math.floor(minutes / 60); const days = Math.floor(hours / 24);
  return minutes < 60 ? `${minutes}m` : hours < 24 ? `${hours}h` : days < 7 ? `${days}d` : days < 28 ? `${Math.floor(days / 7)}w` : `${Math.max(1, Math.floor(days / 30))}mo`;
}
export function AgentPanel({ workspace, visible, activeThreadId, onOpen }: {
  workspace: string; visible: boolean; activeThreadId?: string;
  onOpen: (session: SessionInfo) => void;
}) {
  const unread = useUnreadAcpThreadIds();
  const graph = useInstanceGraph(visible);
  const childrenIndex = useMemo(() => indexTerminalChildren([], graph.items), [graph.items]);
  const { confirm, confirmationDialog } = useConfirmation();
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const { project, scopes, sorts, setScope, setSort } = useProjectAcpPanel();
  const navigate = useAowTabNavigation();
  const worktrees = project.builtin ? undefined : project.worktrees;
  const paths = worktrees && (scopes.user || scopes.cli) ? worktrees.map(worktree => worktree.path) : [workspace];
  const synced = useProjectSessions(paths, visible);
  const local = synced.data.filter(session => session.cwd === workspace);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [opening, setOpening] = useState('');
  const [search, setSearch] = useState('');
  const [refresh, setRefresh] = useState(0);
  useEffect(() => { setImporting(false); }, [workspace, visible]);
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
    try { await acpApi.remove(session.thread_id); removeAcpSessionTabs(session.cwd, session.thread_id); }
    catch (reason) { setError(failure(reason)); } finally { setOpening(''); }
  };
  const open = async (session: SessionInfo) => {
    if (session.cwd === workspace) { onOpen(session); return; }
    setOpening(session.thread_id); setError('');
    try { await navigate({ type: 'acp', thread_id: session.thread_id }); }
    catch (reason) { setError(failure(reason)); }
    finally { setOpening(''); }
  };
  const matches = synced.data.filter(session => (session.cwd === workspace || scopes[session.source === 'cli' ? 'cli' : 'user'])
    && `${session.title} ${session.agent_id}`.toLocaleLowerCase().includes(search.toLocaleLowerCase()))
    .sort(compareSessionActivity);
  const renderSession = (session: SessionInfo) => <InstanceDescendants key={session.thread_id} roots={[{ kind: 'acp', id: session.thread_id }]} title={session.title}
    index={childrenIndex} worktrees={worktrees} detectedAgents={{}} titles={{}} unreadTerminalIds={new Set()}>
    <AowListRow className={`acp-panel-row${activeThreadId === session.thread_id ? ' active' : ''}`}
      title={session.title} tooltip={session.title} disabled={!!opening} active={activeThreadId === session.thread_id}
      openLabel={`打开 ACP 会话 ${session.title}`} onOpen={() => { void open(session); }} menuLabel="会话操作"
      icon={<AgentIcon agentId={session.agent_id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />}
      titlePrefix={session.status === 'working' ? <><span role="status" aria-label="Agent 正在处理…"><SpinnerLabel /></span>{' '}</> : undefined}
      actions={<AowIconButton className="aow-list-row-menu" title="删除会话" aria-label={`删除 ACP 会话 ${session.title}`} disabled={!!opening}
        onClick={() => { void remove(session); }}><Trash2 /></AowIconButton>}>
      <small title={`${session.agent_id} · ${sessionDisplayTime(session)}`}>{timestamp(sessionDisplayTime(session)) || session.agent_id}</small>
      {unread.has(session.thread_id) && <TaskCompletion />}
    </AowListRow>
  </InstanceDescendants>;
  return <section className="acp-history-panel" aria-label="ACP 会话列表">
    <div className="acp-session-search"><Search size={13} /><input aria-label="搜索 ACP 会话" placeholder="搜索会话…" value={search} onChange={event => setSearch(event.target.value)} /></div>
    {(error || graph.error) && <p className="acp-error" role="alert">{error || graph.error}</p>}
    <InstancePanelGroups groups={[
      { id: 'user', title: 'User ACP', icon: <UserRound />, tooltip: '用户创建或导入', items: matches.filter(session => session.source !== 'cli'),
        ...synced.statusFor(workspace, scopes.user),
        actions: <AowIconButton title="导入 ACP 会话" aria-label="导入 ACP 会话" disabled={busy} onClick={() => setImporting(true)}><Download /></AowIconButton> },
      { id: 'cli', title: 'CLI ACP', icon: <SquareTerminal />, tooltip: <><code>aow-cli acp</code> 创建</>, items: matches.filter(session => session.source === 'cli'),
        ...synced.statusFor(workspace, scopes.cli) },
    ]} worktrees={worktrees} activeWorktreePath={workspace} showAll={scopes} sortBy={sorts}
      onShowAllChange={setScope} onSortChange={setSort}
      onReload={() => { synced.reload(); graph.reload(); setRefresh(value => value + 1); }}
      noun="ACP 会话" empty={search ? '没有匹配的会话。' : '暂无会话'}
      itemKey={session => session.thread_id} itemWorkspace={session => session.cwd}
      itemCreatedAt={session => { const time = Date.parse(session.created_at); return Number.isFinite(time) ? time : Number.MAX_SAFE_INTEGER; }} renderItem={renderSession} />
    {importing && <ThreadImport workspace={workspace} agents={agents} existing={local} onClose={() => setImporting(false)} />}
    {confirmationDialog}
  </section>;
}
