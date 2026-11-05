import { useMemo, useState } from 'react';
import { ChevronRight, Download, LoaderCircle, MessageSquare, RefreshCw, Search, Trash2 } from 'lucide-react';
import { t } from '../../lib/i18n';
import { useAowTabNavigation } from '../../aow/tabNavigation';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { AowListRow } from '../../components/AowListRow';
import { AowIconButton } from '../../components/AowIconButton';
import { useConfirmation } from '../../components/ConfirmationDialog';
import { TaskCompletion } from '../../components/TaskCompletion';
import { AgentIcon } from '../agents/AgentIcon';
import { acpApi, AcpThreadImport, removeAcpSessionTabs, sessionDisplayTime, useAcpSessions, type AcpSessionInfo } from '../acp';
import { instanceKey, type InstanceInfo } from '../instances/types';
import { useInstanceGraph } from '../instances/useInstanceGraph';
import { useUnreadAcpThreadIds, useUnreadTerminalIds } from '../notifications/taskNotifications';
import { useTaskScope } from '../tasks/TaskScope';
import { useTerminalState } from '../terminals/useTerminalState';
import { terminalStore } from '../terminals/terminalStore';
import { terminalPaneAgent, terminalPaneTitle, terminalTabPresentation } from '../terminals/terminalPresentation';
import { requestTerminalPaneSelection } from '../terminals/terminalViewState';
import type { TerminalTab } from '../terminals/types';
import { conversationTree, searchConversations, type ConversationNode } from './tree';
import './conversation.css';

function timestamp(value: string) {
  const minutes = Math.max(1, Math.floor((Date.now() - Date.parse(value)) / 60000));
  if (!Number.isFinite(minutes)) return '';
  const hours = Math.floor(minutes / 60), days = Math.floor(hours / 24);
  return minutes < 60 ? `${minutes}m` : hours < 24 ? `${hours}h` : days < 7 ? `${days}d` : days < 28 ? `${Math.floor(days / 7)}w` : `${Math.max(1, Math.floor(days / 30))}mo`;
}

function Row({ node, sessions, activeTerminalId, activeThreadId, unreadTerminals, unreadAcp, onOpen, onRemove, searching }: {
  node: ConversationNode; sessions: Map<string, AcpSessionInfo>; activeTerminalId?: string; activeThreadId?: string;
  unreadTerminals: ReadonlySet<string>; unreadAcp: ReadonlySet<string>; searching: boolean;
  onOpen: (instance: InstanceInfo) => void; onRemove: (instance: InstanceInfo) => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const { instance, children } = node;
  const scope = useTaskScope();
  const session = instance.kind === 'acp' ? sessions.get(instance.id) : undefined;
  const title = (instance.kind === 'acp' ? session?.title : undefined) || instance.title;
  const active = !instance.deleted && (instance.kind === 'acp' ? activeThreadId === instance.id : !!activeTerminalId && activeTerminalId === instance.tab_id);
  const unread = instance.kind === 'acp' ? unreadAcp.has(instance.id) : !!instance.tab_id && unreadTerminals.has(instance.tab_id);
  const open = expanded || searching;
  return <li data-instance={instanceKey(instance)} data-deleted={instance.deleted || undefined}>
    <div className="conversation-row">
      {children.length ? <button type="button" className="conversation-toggle" aria-expanded={open}
        aria-label={t('terminals.child_instances_of', { value0: open ? t('aow.collapse') : t('aow.expand'), value1: title })}
        onClick={() => setExpanded(value => !value)}><ChevronRight className={open ? 'expanded' : undefined} /></button> : <span className="conversation-spacer" />}
      <AowListRow className={`conversation-instance${active ? ' active' : ''}`} active={active} title={title + (instance.deleted ? t('common.deleted') : '')}
        tooltip={title} disabled={instance.deleted} onOpen={() => onOpen(instance)}
        openLabel={instance.kind === 'acp' ? t('acp.open_acp_session', { value0: title }) : title}
        icon={<AgentIcon agentId={instance.agent_id?.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />}
        titlePrefix={session?.status === 'working' ? <span role="status" aria-label={t('aow.agent_is_working')}><LoaderCircle className="spinning" /></span> : undefined}
        menuLabel={t('acp.session_actions')}
        actions={instance.kind === 'acp' && !instance.deleted ? <AowIconButton className="aow-list-row-menu"
          title={t('acp.delete_session_2')} aria-label={t('acp.delete_acp_session_2', { value0: title })} onClick={() => onRemove(instance)}><Trash2 /></AowIconButton> : <span />}>
        <small title={session ? `${session.agent_id} · ${sessionDisplayTime(session)}` : instance.cwd}>
          {session ? `${session.thread_id === scope?.task.initial_thread_id ? t('tasks.main_conversation') + ' · ' : ''}${timestamp(sessionDisplayTime(session))}` : instance.kind === 'acp' ? 'ACP' : t('conversations.terminal_agent')}
        </small>
        {unread && !instance.deleted && <TaskCompletion />}
      </AowListRow>
    </div>
    {children.length && open ? <ul>{children.map(child => <Row key={instanceKey(child.instance)} node={child} sessions={sessions}
      activeTerminalId={activeTerminalId} activeThreadId={activeThreadId} unreadTerminals={unreadTerminals} unreadAcp={unreadAcp}
      onOpen={onOpen} onRemove={onRemove} searching={searching} />)}</ul> : null}
  </li>;
}

export function ConversationPanel({ workspace, visible, embedded = false, activeTerminalId, activeThreadId, onOpenTerminal, onOpenAcp }: {
  workspace: string; visible: boolean; embedded?: boolean; activeTerminalId?: string; activeThreadId?: string;
  onOpenTerminal: (tab: TerminalTab) => void; onOpenAcp: (session: AcpSessionInfo) => void;
}) {
  const taskScope = useTaskScope();
  const graph = useInstanceGraph(visible);
  const terminals = useTerminalState(visible);
  const navigate = useAowTabNavigation();
  const unreadTerminals = useUnreadTerminalIds();
  const unreadAcp = useUnreadAcpThreadIds();
  const { confirm, confirmationDialog } = useConfirmation();
  const [error, setError] = useState('');
  const [search, setSearch] = useState('');
  const [importAgents, setImportAgents] = useState<Awaited<ReturnType<typeof acpApi.agents>> | null>(null);
  const instances = useMemo(() => {
    const items = new Map(graph.items.map(info => [instanceKey(info), info]));
    const livePanes = new Set(terminals.tabs?.flatMap(tab => tab.panes.map(pane => pane.id)));
    // The terminal inventory is authoritative, including immediately after deletion.
    if (terminals.tabs) for (const [key, info] of items) if (info.kind === 'terminal' && !livePanes.has(info.id)) items.set(key, { ...info, deleted: true });
    for (const tab of terminals.tabs ?? []) for (const pane of tab.panes) {
      const agent = terminalPaneAgent(pane, terminals.metadata.agents) || pane.agent_id;
      items.set(`terminal:${pane.id}`, { kind: 'terminal', id: pane.id, parent: pane.parent ?? null, tab_id: tab.id,
        title: tab.panes.length === 1 ? terminalTabPresentation(tab, terminals.metadata.agents, terminals.metadata.titles).title : terminalPaneTitle(pane, terminals.metadata.agents, terminals.metadata.titles, terminals.metadata.activity, terminals.metadata.processes),
        cwd: tab.workspace_root, agent_id: agent ?? null, deleted: false, url: null });
    }
    return [...items.values()];
  }, [graph.items, terminals.tabs, terminals.metadata]);
  const tree = conversationTree(instances, workspace, taskScope?.owns);
  const paths = new Set([workspace]);
  const visit = (nodes: ConversationNode[]) => { for (const node of nodes) { if (node.instance.kind === 'acp' && !node.instance.deleted) paths.add(node.instance.cwd); visit(node.children); } };
  visit(tree);
  const synced = useAcpSessions([...paths], visible);
  const sessions = new Map(synced.data.map(session => [session.thread_id, session]));
  const combined = new Map(instances.map(info => [instanceKey(info), info]));
  for (const session of synced.data) combined.set(`acp:${session.thread_id}`, { kind: 'acp', id: session.thread_id, parent: session.parent ?? null,
    title: session.title, cwd: session.cwd, agent_id: session.agent_id, deleted: false, url: null });
  const terminalCreatedAt = new Map(terminals.tabs?.map(tab => [tab.id, typeof tab.created_at === 'number' ? tab.created_at : Date.parse(tab.created_at ?? '') || 0]));
  const activity = (info: InstanceInfo) => {
    const session = info.kind === 'acp' ? sessions.get(info.id) : undefined;
    return session ? Date.parse(sessionDisplayTime(session)) || 0 : terminalCreatedAt.get(info.tab_id ?? '') ?? 0;
  };
  const nodes = searchConversations(conversationTree([...combined.values()].sort((a, b) => activity(b) - activity(a)), workspace, taskScope?.owns), search);
  const status = synced.statusFor(workspace, true);
  const run = (action: () => Promise<void>) => { setError(''); void action().catch(reason => setError(reason instanceof Error ? reason.message : String(reason))); };
  const open = (instance: InstanceInfo) => run(async () => {
    if (instance.kind === 'terminal') {
      const tab = terminals.tabs?.find(tab => tab.id === instance.tab_id);
      if (tab) { requestTerminalPaneSelection(tab.id, instance.id); onOpenTerminal(tab); }
    } else {
      const session = sessions.get(instance.id);
      if (session && session.cwd === workspace) onOpenAcp(session);
      else await navigate({ type: 'acp', thread_id: instance.id, ...(taskScope ? { workTaskId: taskScope.task.id } : {}) });
    }
  });
  const remove = (instance: InstanceInfo) => run(async () => {
    if (!await confirm({ title: t('acp.delete_acp_session'), description: t('acp.delete_session', { value0: instance.title }),
      warning: t('acp.for_agents_supporting_remote_deletion_the_remote_record_will'), confirmLabel: t('acp.delete'), danger: true })) return;
    await acpApi.remove(instance.id);
    removeAcpSessionTabs(instance.cwd, instance.id);
    synced.reload(); graph.reload();
  });
  const searchInput = <div className="conversation-search"><Search size={13} /><input aria-label={t('conversations.search')} placeholder={t('conversations.search')}
    value={search} onChange={event => setSearch(event.target.value)} /></div>;
  const actions = <><AowIconButton title={t('acp.import_acp_sessions')} aria-label={t('acp.import_acp_sessions')}
    onClick={() => run(async () => setImportAgents(await acpApi.agents()))}><Download /></AowIconButton>
    <AowIconButton title={t('conversations.refresh')} aria-label={t('conversations.refresh')} onClick={() => { graph.reload(); synced.reload(); void terminalStore.refresh(); }}><RefreshCw /></AowIconButton></>;
  const content = <>
    {(error || graph.error || status.error || terminals.error) && <p role="alert">{error || graph.error || status.error || terminals.error}</p>}
    {!nodes.length && <p className="conversation-empty">{status.loading ? t('conversations.loading') : t('conversations.empty')}</p>}
    <ul className="conversation-tree">{nodes.map(node => <Row key={instanceKey(node.instance)} node={node} sessions={sessions}
      activeTerminalId={activeTerminalId} activeThreadId={activeThreadId} unreadTerminals={unreadTerminals} unreadAcp={unreadAcp}
      onOpen={open} onRemove={remove} searching={!!search.trim()} />)}</ul>
  </>;
  return <section className={`conversation-panel${embedded ? ' conversation-panel-embedded' : ''}`} aria-label={t('navigation.conversation')}>
    {embedded ? <><div className="conversation-toolbar">{searchInput}{actions}</div>{content}</> : <>
      {searchInput}<AowPanelStack><AowPanel title={t('navigation.conversation')} icon={<MessageSquare />} collapsible={false} actions={actions}>{content}</AowPanel></AowPanelStack>
    </>}
    {importAgents && <AcpThreadImport workspace={workspace} agents={importAgents} existing={synced.data.filter(session => session.cwd === workspace)} onClose={() => setImportAgents(null)} />}
    {confirmationDialog}
  </section>;
}
