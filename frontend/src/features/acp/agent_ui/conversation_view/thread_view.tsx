// React rendering of the ACP branches in thread_view.rs, with independent view state.
import { useCallback, useContext, useMemo, useRef, useState, type ReactNode } from 'react';
import { Maximize2, Minimize2, Send, Square } from 'lucide-react';
import { ConversationAgentMessage, ConversationLayout } from '../../../../components/conversation/ConversationLayout';
import { AgentIcon } from '../../../agents/AgentIcon';
import { conversationTurns, overviewPrompt, turnPresentation } from './conversation_turns';
import { OverviewResponse, TurnResponse } from './turn_response';
import type { OpenFile } from './content';
import { useEntryRenderer } from './entry_renderer';
import { PermissionRequest } from './elicitation';
import { Notice } from './activity';
import { SessionMeta } from './session_meta';
import { ReplyActions } from './reply_actions';
import { Plan } from './plan';
import { absoluteFilePath } from './file_links';
import { WorkspaceMarkdownProvider } from '../../../files/WorkspaceMarkdown';
import { ConfigOptions } from '../config_options';
import { useComposerDraft } from '../composer_drafts';
import { MessageEditor } from '../message_editor';
import { emptyDraft, hasDraftContent, hasImages, hasPendingImages, MAX_PROMPT_BYTES, promptContent } from '../message_editor/model';
import { messageGroups, visibleMessageGroups } from './entry_view_state';
import { ThinkingVisibility } from '../thinking_visibility';
import { list, object, text, type Data, type SessionSnapshot, type TurnOverview } from '../../types';
import { SpinnerLabel } from '../ui/spinner_label';

function TokenUsage({ usage }: { usage: Data | null }) {
  if (!usage) return null;
  const used = Number(usage.used ?? usage.totalTokens ?? 0); const size = Number(usage.size ?? usage.contextWindow ?? 0);
  if (!size) return null;
  const percent = Math.round(used / size * 100); const cost = object(usage.cost);
  const title = `Context: ${percent}% · ${used.toLocaleString()} / ${size.toLocaleString()}${typeof cost.amount === 'number' ? `\nCost: ${cost.amount.toFixed(cost.amount > 0 && cost.amount < .01 ? 4 : 2)} ${text(cost.currency)}` : ''}`;
  return <span className={`acp-token-usage${percent >= 85 ? ' warning' : ''}`} role="img" aria-label={title} title={title}><svg width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" opacity=".25" /><circle cx="8" cy="8" r="6" pathLength="100" strokeDasharray={`${Math.min(100, Math.max(0, percent))} 100`} transform="rotate(-90 8 8)" /></svg></span>;
}
export function ThreadView({ session, workspace, tabId, busy: pending, supportsImages = false, supportsSteering = false, composerStatus, headerActions, notice, alternateContent, onAction, onOpenFile: openHostFile }: { session: SessionSnapshot; workspace: string; tabId: string; busy: boolean; supportsImages?: boolean; supportsSteering?: boolean; composerStatus?: ReactNode; headerActions?: ReactNode; notice?: ReactNode; alternateContent?: ReactNode; onAction: (action: Data) => Promise<boolean>; onOpenFile: OpenFile }) {
  const busy = pending || !!composerStatus;
  const showThinking = useContext(ThinkingVisibility);
  const [draft, setDraft] = useComposerDraft(workspace, tabId);
  const [composeError, setComposeError] = useState('');
  const [expanded, setExpanded] = useState(false);
  const [followRequest, setFollowRequest] = useState(0);
  const conversation = useRef<HTMLDivElement>(null);
  const working = session.status === 'working';
  const onOpenFile: OpenFile = useCallback((path, line) => openHostFile(absoluteFilePath(path, session.cwd || workspace), line), [openHostFile, session.cwd, workspace]);
  const toolId = (permission: typeof session.permissions[number]) => text(object(permission.request.toolCall).toolCallId);
  const permissionTools = useMemo(() => new Set(session.permissions.map(permission => text(object(permission.request.toolCall).toolCallId))), [session.permissions]);
  const groups = useMemo(() => visibleMessageGroups(messageGroups(session.entries, working, session.permissions.length > 0, permissionTools, new Set(session.turns?.map(turn => turn.user_entry_id))), showThinking), [session.entries, working, session.permissions.length, permissionTools, session.turns, showThinking]);
  const { leading, turns } = useMemo(() => conversationTurns(groups), [groups]);
  const entryPositions = useMemo(() => new Map(session.entries.map((entry, index) => [entry.id, index])), [session.entries]);
  const turnRecords = useMemo(() => new Map(session.turns?.map(turn => [turn.user_entry_id, turn])), [session.turns]);
  const agentIcon = <AgentIcon agentId={session.agent_id.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} />;
  const canSteer = working && supportsSteering;
  const canSend = !busy && !session.steering_pending && (session.status === 'idle' || canSteer) && hasDraftContent(draft) && !hasPendingImages(draft) && (!hasImages(draft) || supportsImages);
  const submit = async () => {
    if (!canSend) return;
    const submitted = draft;
    const action = { action: canSteer ? 'steer' : 'prompt', content: promptContent(submitted) };
    if (new Blob([JSON.stringify(action)]).size > MAX_PROMPT_BYTES) { setComposeError('Message exceeds 32 MiB. Remove some images and try again.'); return; }
    setComposeError('');
    if (await onAction(action)) {
      // Image conversion updates the cache but keeps the document identity. Only
      // user edits replace parts; a removed image finishing must not retain a sent draft.
      setDraft(current => current.parts === submitted.parts ? emptyDraft() : current); setExpanded(false); setFollowRequest(value => value + 1);
    }
  };
  const tools = new Set(session.overview ? session.overview.flatMap(turn => turn.permission_tool_ids) : session.entries.filter(entry => entry.kind === 'tool').map(entry => entry.id));
  const plan = list(session.plan?.entries);
  const hasMessages = !!session.overview?.length || groups.length > 0 || working;
  // Upstream appends its generating indicator to the transcript, after the last entry.
  const confirmation = session.permissions.length > 0;
  const compacting = session.overview ? session.overview.some(turn => turn.compacting) : session.entries.some(entry => entry.kind === 'compaction_update' && entry.content.status === 'in_progress');
  const generating = working && !compacting ? <div className="acp-content acp-working" role="status" aria-label={confirmation ? 'Awaiting Confirmation' : 'Agent 正在处理…'}>
    <SpinnerLabel variant={confirmation ? 'sand' : 'dots'} />{confirmation && <span>Awaiting Confirmation</span>}
  </div> : null;
  const renderEntry = useEntryRenderer(session, busy, onAction, onOpenFile);
  const renderGroup = (group: typeof groups[number], controls = false) => renderEntry(group,
    controls && group.showControls && <ReplyActions content={group.reply} onJump={() => {
      const userMessageId = group.userMessageId ?? [...groups].reverse().find(item => item.kind === 'user')?.id;
      const target = Array.from(conversation.current?.querySelectorAll<HTMLElement>('[data-entry-id]') ?? []).find(element => element.dataset.entryId === userMessageId);
      if (target) target.scrollIntoView({ block: 'start', behavior: 'instant' });
      else conversation.current?.scrollTo({ top: conversation.current.scrollHeight, behavior: 'instant' });
    }} onScrollToTop={group === groups.at(-1) ? () => conversation.current?.scrollTo({ top: 0, behavior: 'instant' }) : undefined} />);

  const renderOverviewResponse = (turn: TurnOverview) => {
    const latest = turn === session.overview?.at(-1);
    return <OverviewResponse permissionTools={permissionTools} renderGroup={group => renderGroup(group)}
      remote={{ session, turn, busy, onAction, onOpenFile }} generating={latest && generating}
      onJump={() => {
        const target = Array.from(conversation.current?.querySelectorAll<HTMLElement>('[data-entry-id]') ?? []).find(element => element.dataset.entryId === turn.id);
        if (target) target.scrollIntoView({ block: 'start', behavior: 'instant' });
        else conversation.current?.scrollTo({ top: conversation.current.scrollHeight, behavior: 'instant' });
      }} onScrollToTop={latest ? () => conversation.current?.scrollTo({ top: 0, behavior: 'instant' }) : undefined} />;
  };
  const projectedLeading = session.overview?.find(turn => !turn.user.length);
  return <WorkspaceMarkdownProvider directory={session.cwd || workspace}><ConversationLayout
    sessionId={session.thread_id}
    revision={session.revision}
    label="ACP 对话"
    header={{ title: session.title, agentName: session.agent_id, agentIcon, workspace: session.cwd, actions: headerActions }}
    alternateContent={alternateContent}
    live
    followRequest={followRequest}
    scrollRef={conversation}
    scrollClassName="acp-conversation"
    leadingContent={session.overview ? projectedLeading ? <ConversationAgentMessage agentName={session.agent_id} agentIcon={agentIcon}>{renderOverviewResponse(projectedLeading)}</ConversationAgentMessage> : !session.overview.length && generating
      : leading.length || working && !turns.length ? <ConversationAgentMessage agentName={session.agent_id} agentIcon={agentIcon}>{leading.map(group => renderGroup(group, true))}{!turns.length && generating}</ConversationAgentMessage> : undefined}
    turns={session.overview ? session.overview.filter(turn => turn.user.length).map(turn => ({
      id: turn.id, prompt: overviewPrompt(turn), status: turn.status ?? undefined,
      user: messageGroups(turn.user, false, false).map(group => renderGroup(group)), assistant: renderOverviewResponse(turn),
    })) : turns.map((turn, index) => {
      const latest = index === turns.length - 1;
      const start = entryPositions.get(turn.id) ?? 0;
      const end = latest ? session.entries.length : entryPositions.get(turns[index + 1].id) ?? session.entries.length;
      const presentation = turnPresentation(turn.response, session.entries.slice(start, end), session, latest, turnRecords.get(turn.id));
      return {
        id: turn.id, prompt: turn.prompt, status: presentation.status,
        user: renderGroup(turn.user), assistant: <TurnResponse presentation={presentation} permissionTools={permissionTools} renderGroup={group => renderGroup(group)} generating={latest && generating}
          onJump={() => {
            const target = Array.from(conversation.current?.querySelectorAll<HTMLElement>('[data-entry-id]') ?? []).find(element => element.dataset.entryId === turn.id);
            target?.scrollIntoView({ block: 'start', behavior: 'instant' });
          }} onScrollToTop={latest ? () => conversation.current?.scrollTo({ top: 0, behavior: 'instant' }) : undefined} />,
      };
    })}
    accessory={
      <div className="acp-content acp-activity">
        <SessionMeta metadata={session.session_meta} />
        {session.notices?.map(notice => <Notice key={notice.id} notice={notice} busy={busy} onDismiss={() => { void onAction({ action: 'dismiss_notice', notice_id: notice.id }); }} />)}
        {session.permissions.filter(permission => !tools.has(toolId(permission))).map(permission => <PermissionRequest key={permission.id} permission={permission} requester={session.agent_id} busy={busy} onAnswer={response => { void onAction({ action: 'answer', request_id: permission.id, response }); }} />)}
        {notice}
        {session.error && !composerStatus && !session.auth_required && <p className="acp-error" role="alert">{session.error}</p>}
        <Plan key={session.thread_id} entries={plan} busy={busy} onClear={() => { void onAction({ action: 'clear_plan' }); }} onOpenFile={onOpenFile} />
      </div>
    }
    footerExpanded={expanded && !composerStatus}
    footer={composerStatus ||
      <form className={`acp-content acp-composer${!hasMessages ? ' empty' : ''}${expanded ? ' expanded' : ''}`} onSubmit={event => { event.preventDefault(); void submit(); }}>
        <div className="acp-editor">
          {hasMessages && <button type="button" className="acp-editor-expand" aria-label={expanded ? '收起输入区' : '展开输入区'} onClick={() => setExpanded(value => !value)}>{expanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}</button>}
          <MessageEditor draft={draft} onChange={setDraft} commands={session.commands} supportsImages={supportsImages} disabled={session.status === 'disconnected'} placeholder={`Message ${session.agent_id}${session.commands.length ? ' — / for commands' : ''}`} onSubmit={() => { void submit(); }} />
        </div>
        {session.steering_untracked && <p className="acp-status" role="status">Agent 已启动后续任务，但尚未提供执行状态。请勿重复发送。</p>}
        {composeError && <p className="acp-error" role="alert">{composeError}</p>}
        <div className="acp-composer-toolbar"><span className="acp-composer-spacer" /><TokenUsage usage={session.usage} /><ConfigOptions session={session} disabled={busy || session.status === 'disconnected'} onAction={onAction} />
          {working && <button className="acp-send-button acp-stop-button aow-icon-inherit" type="button" aria-label="停止" title="停止" disabled={busy} onClick={() => { void onAction({ action: 'cancel' }); }}><Square size={14} fill="currentColor" strokeWidth={0} /></button>}
          {(!working || canSteer) && <button className="acp-send-button" type="submit" aria-label={canSteer ? '追加指令' : '发送'} title={canSteer ? '追加到当前任务 · Enter（Shift + Enter 换行）' : '发送 · Enter（Shift + Enter 换行）'} disabled={!canSend}><Send size={16} /></button>}
        </div>
      </form>
    }
  /></WorkspaceMarkdownProvider>;
}
