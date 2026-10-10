// React rendering of the ACP branches in thread_view.rs, with independent view state.
import { useEffect, useRef, useState } from 'react';
import { ArrowDown, Brain, Check, ChevronDown, ChevronRight, LoaderCircle, Maximize2, Minimize2, Send, Square } from 'lucide-react';
import { ContentBlock, ToolCall, type OpenFile } from './content';
import { PermissionRequest } from './elicitation';
import { Compaction, Notice } from './activity';
import { ReplyActions } from './reply_actions';
import { absoluteFilePath } from './file_links';
import { ConfigOptions } from '../config_options';
import { useComposerDraft } from '../composer_drafts';
import { messageGroups, useEntryViewState } from './entry_view_state';
import { list, object, text, type Data, type SessionSnapshot } from '../../types';

function TokenUsage({ usage }: { usage: Data | null }) {
  if (!usage) return null;
  const used = Number(usage.used ?? usage.totalTokens ?? 0); const size = Number(usage.size ?? usage.contextWindow ?? 0);
  if (!size) return null;
  const percent = Math.round(used / size * 100); const cost = object(usage.cost);
  const title = `Context: ${percent}% · ${used.toLocaleString()} / ${size.toLocaleString()}${typeof cost.amount === 'number' ? `\nCost: ${cost.amount.toFixed(cost.amount > 0 && cost.amount < .01 ? 4 : 2)} ${text(cost.currency)}` : ''}`;
  return <span className={`zed-token-usage${percent >= 85 ? ' warning' : ''}`} role="img" aria-label={title} title={title}><svg width="16" height="16" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6" opacity=".25" /><circle cx="8" cy="8" r="6" pathLength="100" strokeDasharray={`${Math.min(100, Math.max(0, percent))} 100`} transform="rotate(-90 8 8)" /></svg></span>;
}
export function ThreadView({ session, workspace, tabId, busy, onAction, onOpenFile: openHostFile }: { session: SessionSnapshot; workspace: string; tabId: string; busy: boolean; onAction: (action: Data) => Promise<boolean>; onOpenFile: OpenFile }) {
  const [prompt, setPrompt] = useComposerDraft(workspace, tabId);
  const [expanded, setExpanded] = useState(false);
  const [commandIndex, setCommandIndex] = useState(0);
  const [following, setFollowing] = useState(true);
  const conversation = useRef<HTMLDivElement>(null);
  const end = useRef<HTMLDivElement>(null);
  const editor = useRef<HTMLTextAreaElement>(null);
  const working = session.status === 'working';
  const onOpenFile: OpenFile = (path, line) => openHostFile(absoluteFilePath(path, session.cwd || workspace), line);
  const toolId = (permission: typeof session.permissions[number]) => text(object(permission.request.toolCall).toolCallId);
  const groups = messageGroups(session.entries, working, session.permissions.length > 0, new Set(session.permissions.map(toolId)));
  const views = useEntryViewState(session.entries, working);
  useEffect(() => { if (following) end.current?.scrollIntoView({ block: 'nearest' }); }, [session.revision, following]);
  const submit = async () => {
    const submitted = prompt;
    if (submitted.trim() && !busy && session.status === 'idle' && await onAction({ action: 'prompt', content: [{ type: 'text', text: submitted }] })) {
      setPrompt(current => current === submitted ? '' : current); setExpanded(false); setFollowing(true);
    }
  };
  const commands = /^\/[^\s]*$/.test(prompt) ? session.commands.filter(command => text(command.name).toLocaleLowerCase().includes(prompt.slice(1).toLocaleLowerCase())) : [];
  const chooseCommand = (name: string) => { setPrompt(`/${name} `); setCommandIndex(0); editor.current?.focus(); };
  const tools = new Set(session.entries.filter(entry => entry.kind === 'tool').map(entry => entry.id));
  const plan = list(session.plan?.entries);
  const hasMessages = groups.length > 0;
  return <>
    {hasMessages && <div className="zed-conversation" ref={conversation} onScroll={event => { const element = event.currentTarget; setFollowing(element.scrollHeight - element.scrollTop - element.clientHeight < 60); }}>
      {groups.map(group => <article key={group.id} data-entry-id={group.id} className={`zed-entry zed-entry-${group.kind}`}>
        {group.kind === 'user' ? <div className="zed-user-message">{group.entries.map(entry => <ContentBlock key={entry.id} content={entry.content} onOpenFile={onOpenFile} user />)}</div>
          : group.kind === 'assistant' ? <>{group.entries.map(entry => {
            if (entry.kind !== 'thought') return <div className="zed-message-chunk" key={entry.id}><ContentBlock content={entry.content} onOpenFile={onOpenFile} /></div>;
            const state = views.thinking(entry.id);
            return <section key={entry.id} className="zed-thinking"><button type="button" aria-label="思考过程" aria-expanded={state.open} onClick={() => views.toggleThinking(entry.id)}><Brain size={14} /><span>Thinking</span>{state.open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}</button>
              {state.open && <div className={`zed-thinking-content${state.constrained ? ' constrained' : ''}`} ref={element => { if (element && state.following) element.scrollTop = element.scrollHeight; }}><ContentBlock content={entry.content} onOpenFile={onOpenFile} /></div>}
            </section>;
          })}</>
            : group.kind === 'tool' ? <ToolCall content={group.entries[0].content} terminals={session.terminals} onOpenFile={onOpenFile} busy={busy} permission={session.permissions.find(permission => toolId(permission) === group.id)} onAnswer={response => { const permission = session.permissions.find(item => toolId(item) === group.id); if (permission) void onAction({ action: 'answer', request_id: permission.id, response }); }} />
              : group.kind === 'summary' ? <ContentBlock content={group.entries[0].content} onOpenFile={onOpenFile} />
                : <Compaction content={group.entries[0].content} onOpenFile={onOpenFile} />}
        {!!group.reply && <ReplyActions content={group.reply} onJump={() => Array.from(conversation.current?.querySelectorAll<HTMLElement>('[data-entry-id]') ?? []).find(element => element.dataset.entryId === group.userMessageId)?.scrollIntoView({ block: 'start', behavior: 'smooth' })} />}
      </article>)}
      <div ref={end} />
    </div>}
    {!following && hasMessages && <button type="button" className="zed-follow" onClick={() => { setFollowing(true); end.current?.scrollIntoView({ block: 'end', behavior: 'smooth' }); }}><ArrowDown size={13} />跳到最新消息</button>}
    <div className="zed-activity">
      {session.notices?.map(notice => <Notice key={notice.id} notice={notice} busy={busy} onDismiss={() => { void onAction({ action: 'dismiss_notice', notice_id: notice.id }); }} />)}
      {session.permissions.filter(permission => !tools.has(toolId(permission))).map(permission => <PermissionRequest key={permission.id} permission={permission} busy={busy} onAnswer={response => { void onAction({ action: 'answer', request_id: permission.id, response }); }} />)}
      {session.error && !session.auth_required && <p className="zed-error" role="alert">{session.error}</p>}
      {working && <p className="zed-working" role="status"><LoaderCircle className="zed-spinner" size={13} />Agent 正在处理…</p>}
      {!!plan.length && <details className="zed-plan"><summary>{plan.filter(item => item.status === 'completed').length} / {plan.length} 项计划已完成</summary><ol>{plan.map((item, index) => <li key={index} data-status={text(item.status)}>{item.status === 'completed' ? <Check size={13} /> : item.status === 'in_progress' ? <LoaderCircle className="zed-spinner" size={13} /> : <span className="zed-plan-dot" />}{text(item.content)}</li>)}</ol></details>}
      {session.status === 'disconnected' && <button type="button" disabled={busy} onClick={() => { void onAction({ action: 'resume' }); }}>恢复会话</button>}
    </div>
    <form className={`zed-composer${!hasMessages ? ' empty' : ''}${expanded ? ' expanded' : ''}`} onSubmit={event => { event.preventDefault(); void submit(); }}>
      <div className="zed-editor">
        {hasMessages && <button type="button" className="zed-editor-expand" aria-label={expanded ? '收起输入区' : '展开输入区'} onClick={() => setExpanded(value => !value)}>{expanded ? <Minimize2 size={14} /> : <Maximize2 size={14} />}</button>}
        {!!commands.length && <div className="zed-command-menu" role="listbox" aria-label="Agent 命令">{commands.map((command, index) => <button type="button" key={text(command.name)} role="option" aria-selected={index === commandIndex % commands.length} onClick={() => chooseCommand(text(command.name))}><strong>/{text(command.name)}</strong><span>{text(command.description)}</span></button>)}</div>}
        <textarea ref={editor} aria-label="ACP 消息" placeholder={`Message ${session.agent_id}${session.commands.length ? ' — / for commands' : ''}`} value={prompt} disabled={session.status === 'disconnected'} onChange={event => { setPrompt(event.target.value); setCommandIndex(0); }} onKeyDown={event => {
          if (event.nativeEvent.isComposing) return;
          if (commands.length && ['ArrowDown', 'ArrowUp'].includes(event.key)) { event.preventDefault(); setCommandIndex(index => (index + (event.key === 'ArrowUp' ? -1 : 1) + commands.length) % commands.length); }
          else if (commands.length && ['Enter', 'Tab'].includes(event.key) && !event.shiftKey) { event.preventDefault(); chooseCommand(text(commands[commandIndex % commands.length].name)); }
          else if (event.key === 'Enter' && !event.shiftKey) { event.preventDefault(); void submit(); }
        }} />
      </div>
      <div className="zed-composer-toolbar"><span className="zed-composer-spacer" /><TokenUsage usage={session.usage} /><ConfigOptions session={session} disabled={busy || session.status === 'disconnected'} onAction={onAction} />
        {working ? <button className="zed-send-button" type="button" aria-label="停止" title="停止" disabled={busy} onClick={() => { void onAction({ action: 'cancel' }); }}><Square size={14} /></button>
          : <button className="zed-send-button" type="submit" aria-label="发送" title="发送 · Enter（Shift + Enter 换行）" disabled={busy || session.status !== 'idle' || !prompt.trim()}><Send size={16} /></button>}
      </div>
    </form>
  </>;
}
