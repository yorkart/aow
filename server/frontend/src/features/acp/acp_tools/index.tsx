import { t } from '../../../lib/i18n';
import { memo, useId, useMemo, useState, type ReactNode } from 'react';
import { ArrowDown, ArrowLeft, ArrowUp, RefreshCw, Trash2, TriangleAlert } from 'lucide-react';
import { refractor } from 'refractor/core';
import json from 'refractor/json';
import type { DebugMessage } from '../types';
import { CopyButton } from '../agent_ui/ui/copy_button';
import { logMessages, payloadText, type LogMessage } from './messages';
import './acp_tools.css';

refractor.register(json);
type Tokens = ReturnType<typeof refractor.highlight>['children'];
function renderTokens(nodes: Tokens): ReactNode {
  return nodes.map((node, index) => {
    if (node.type === 'text') return node.value;
    if (node.type !== 'element') return null;
    const classes = node.properties.className;
    return <span key={index} className={Array.isArray(classes) ? classes.join(' ') : undefined}>{renderTokens(node.children)}</span>;
  });
}

function JsonCode({ text, raw }: { text: string; raw: boolean }) {
  const highlighted = useMemo(() => {
    if (raw || text.length > 65_536) return text;
    try { return renderTokens(refractor.highlight(text, 'json').children); }
    catch { return text; }
  }, [text, raw]);
  return <code>{highlighted}</code>;
}

const LogRow = memo(function LogRow({ message }: { message: LogMessage }) {
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  const { source, requestId } = message;
  const text = payloadText(message, expanded);
  const direction = source.direction === 'outgoing' ? t("acp.outgoing") : source.direction === 'incoming' ? t("acp.incoming") : t("acp.standard_error");
  const Icon = source.direction === 'outgoing' ? ArrowUp : source.direction === 'incoming' ? ArrowDown : TriangleAlert;
  const toggle = () => setExpanded(value => !value);
  return <article className="acp-log-row" data-direction={source.direction} data-expanded={expanded} data-error={message.failed}>
    <button type="button" className="acp-log-heading" onClick={toggle} aria-expanded={expanded} aria-controls={bodyId} title={`${direction} · ${source.timestamp}`}>
      <Icon size={15} className="acp-log-direction" aria-label={direction} />
      <span className="acp-log-method">{message.name}</span>
      <span className="acp-log-badges">
        {source.scope === 'connection' && <span className="acp-log-badge acp-log-shared">{t("acp.shared_connection")}</span>}
        <span className={`acp-log-badge${requestId !== undefined ? ' acp-log-type' : ''}`}>{message.type}</span>
        {requestId !== undefined && <span className="acp-log-badge acp-log-id" title={String(requestId)}>{requestId}</span>}
      </span>
    </button>
    <div id={bodyId} className="acp-log-body">
      {text !== undefined && (expanded ? <div className="acp-log-expanded">
        <pre className="acp-log-json"><JsonCode text={text} raw={message.raw !== undefined} /></pre>
        <CopyButton text={text} label={t("navigation.copy_message")} />
      </div> : <div className="acp-log-preview-scroll"><button type="button" className="acp-log-preview acp-log-json" onClick={toggle} aria-expanded={false} aria-label={t("acp.expand_tool_json", { name: message.name })}><JsonCode text={text} raw={message.raw !== undefined} /></button></div>)}
    </div>
  </article>;
});

export function AcpLogs({ entries, threadId, agentName, busy, onRefresh, onBack }: {
  entries: DebugMessage[]; threadId: string; agentName: string; busy: boolean; onRefresh: () => void; onBack: () => void;
}) {
  const messages = useMemo(() => logMessages(entries), [entries]);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const shown = useMemo(() => messages.filter(message => !hidden.has(message.key)), [messages, hidden]);
  const copied = useMemo(() => JSON.stringify(shown.map(message => ({
    _direction: message.source.direction, _type: message.type.toLowerCase(),
    id: message.requestId, method: message.name, params: message.raw ?? message.payload ?? null,
    connection_id: message.source.connection_id, thread_id: message.source.thread_id,
    session_id: message.source.session_id, scope: message.source.scope, timestamp: message.source.timestamp,
  })), null, 2), [shown]);
  return <section className="acp-content acp-logs" aria-label={t("acp.acp_protocol_logs")}>
    <header className="acp-logs-toolbar">
      <button type="button" className="acp-quiet-button" onClick={onBack} aria-label={t("acp.back_to_session")} title={t("acp.back_to_session")}><ArrowLeft size={15} /></button>
      <strong>{agentName}</strong><span className="acp-logs-count" title={t("acp.thread_in_memory_logs_up_to_500_entries_per", { value0: threadId })}>{shown.length}</span>
      <div className="acp-logs-actions">
        <button type="button" className="acp-quiet-button" onClick={onRefresh} disabled={busy} aria-label={t("navigation.refresh_messages")} title={t("navigation.refresh_messages")}><RefreshCw size={15} className={busy ? 'acp-spinner' : undefined} /></button>
        {!!shown.length && <CopyButton text={copied} label={t("navigation.copy_all_messages")} />}
        <button type="button" className="acp-quiet-button" onClick={() => setHidden(new Set(messages.map(message => message.key)))} disabled={!shown.length} aria-label={t("navigation.clear_messages")} title={t("navigation.clear_messages")}><Trash2 size={15} /></button>
      </div>
    </header>
    <div className="acp-logs-list">
      {shown.map(message => <LogRow key={message.key} message={message} />)}
      {!shown.length && <p className="acp-logs-empty">{t("navigation.no_messages_recorded_yet")}</p>}
    </div>
  </section>;
}
