import { t } from '../../../../lib/i18n';
import type { ReactNode } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { ContentBlock, type OpenFile } from './content';
import { ToolEntry } from './tool_call';
import { Compaction } from './activity';
import { useEntryViewState, type MessageGroup } from './entry_view_state';
import { object, text, type Data, type SessionSnapshot } from '../../types';

export function useEntryRenderer(session: SessionSnapshot, busy: boolean, onAction: (action: Data) => Promise<boolean>, onOpenFile: OpenFile) {
  const views = useEntryViewState(session.entries, session.status === 'working');
  const toolId = (permission: typeof session.permissions[number]) => text(object(permission.request.toolCall).toolCallId);
  return (group: MessageGroup, footer?: ReactNode) => <article key={group.id} data-entry-id={group.id} className={`acp-content acp-entry acp-entry-${group.kind}`}>
    {['user', 'steering'].includes(group.kind) ? <div className="acp-user-message">{group.entries.map(entry => <div key={entry.id}>{entry.delivery && <p className={entry.delivery.status === 'failed' || entry.delivery.status === 'unknown' ? 'acp-error' : 'acp-status'} role="status">{t("acp.additional_instructions")} {({ sending: t("acp.sending"), injected: t("acp.submitted"), started: t("acp.follow_up_task_started"), failed: t("acp.failed_to_send"), cancelled: t("acp.canceled"), unknown: t("acp.delivery_status_unknown") })[entry.delivery.status]}{entry.delivery.error && `：${entry.delivery.error}`}</p>}<ContentBlock content={entry.content} onOpenFile={onOpenFile} user /></div>)}</div>
      : group.kind === 'assistant' ? <>{group.entries.map(entry => {
        if (entry.kind !== 'thought') return <div className="acp-message-chunk" key={entry.id}><ContentBlock content={entry.content} onOpenFile={onOpenFile} message /></div>;
        const state = views.thinking(entry.id);
        return <section key={entry.id} className="acp-thinking"><button type="button" aria-label={t("acp.thinking")} aria-expanded={state.open} onClick={() => views.toggleThinking(entry.id)}><span>{t("navigation.thinking")}</span>{state.open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}</button>
          {state.open && <div className={`acp-thinking-content${state.constrained ? ' constrained' : ''}`} ref={element => { if (element && state.following) element.scrollTop = element.scrollHeight; }}><ContentBlock content={entry.content} onOpenFile={onOpenFile} /></div>}
        </section>;
      })}</>
        : group.kind === 'tool' ? <ToolEntry content={group.entries[0].content} terminals={session.terminals} onOpenFile={onOpenFile} busy={busy} permission={session.permissions.find(permission => toolId(permission) === group.id)} onAction={onAction} />
          : group.kind === 'summary' ? <ContentBlock content={group.entries[0].content} onOpenFile={onOpenFile} />
            : <Compaction content={group.entries[0].content} onOpenFile={onOpenFile} />}
    {footer}
  </article>;
}
