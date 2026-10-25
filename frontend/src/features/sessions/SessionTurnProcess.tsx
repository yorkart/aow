import { t } from '../../lib/i18n';
import { useEffect, useId, useState } from 'react';
import { Check, ChevronRight, Circle, CircleAlert, LoaderCircle, MessageSquare, Minus, Wrench } from 'lucide-react';
import type { AgentSessionActivity, AgentSessionSnapshotTurn, AgentSessionToolText } from './types';
import { activityTimeLabel, groupSessionActivities, toolGroupStatus, toolSummary } from './sessionProcessPresentation';
import { ConversationProcess } from '../../components/conversation/ConversationProcess';
import './session-turn-process.css';

const statusLabels = { completed: t("aow.completed"), failed: t("aow.failed"), interrupted: t("aow.interrupted"), in_progress: t("aow.running"), unknown: t("sessions.result_not_recorded") };

function ActivityTimeMarker({ activities, kind }: { activities: AgentSessionActivity[]; kind: AgentSessionActivity['kind'] }) {
  const label = activityTimeLabel(activities);
  const description = kind === 'commentary' ? t("sessions.progress_timestamp") : activities.length > 1 ? t("sessions.tool_group_timestamp") : t("sessions.tool_call_timestamp");
  return <span className="session-process-marker" title={label} role="img" aria-label={`${description}：${label}`} tabIndex={0}>
    {kind === 'tool' ? <Wrench size={12} aria-hidden="true" /> : <MessageSquare size={12} aria-hidden="true" />}
  </span>;
}

function ToolStatus({ status, label = statusLabels[status ?? 'unknown'] }: { status: AgentSessionActivity['status']; label?: string }) {
  const Icon = status === 'completed' ? Check : status === 'failed' ? CircleAlert
    : status === 'in_progress' ? LoaderCircle : status === 'interrupted' ? Minus : Circle;
  return <span className={`session-process-status ${status ?? 'unknown'}`} title={label}>
    <Icon size={13} className={status === 'in_progress' ? 'spinning' : undefined} aria-hidden="true" />
    <span>{label}</span>
  </span>;
}

function ToolText({ label, value, error = false }: { label: string; value?: AgentSessionToolText; error?: boolean }) {
  if (!value) return null;
  return <div className={`session-tool-text${error ? ' error' : ''}`}>
    <div className="session-tool-text-label">{label}{value.truncated && <span>{t("sessions.content_too_long_only_the_beginning_and_end_are")}</span>}</div>
    <pre tabIndex={0} aria-label={label}>{value.text}</pre>
  </div>;
}

function ToolRow({ tool }: { tool: AgentSessionActivity }) {
  const [expanded, setExpanded] = useState(false);
  const contentId = useId();
  const details = tool.details;
  const preview = (details?.command?.text ?? details?.input?.text)?.replace(/\s+/g, ' ').slice(0, 240);
  const heading = <>
    <span>{toolSummary([tool])}</span>
    <code>{tool.text}</code>
    <ToolStatus status={tool.status} />
  </>;
  if (!details) return <div className="session-process-tool">{heading}</div>;
  return <div className={`session-process-tool has-details${tool.status === 'failed' ? ' failed' : ''}`}>
    <button type="button" className="session-tool-toggle" aria-expanded={expanded} aria-controls={contentId}
      aria-label={t("sessions.tool_call_details", { value0: expanded ? t("aow.collapse") : t("aow.expand"), value1: tool.text })} onClick={() => setExpanded(!expanded)}>
      {heading}
      <ChevronRight className="session-tool-chevron" size={13} aria-hidden="true" />
    </button>
    {preview && <code className="session-tool-preview">{preview}</code>}
    <div id={contentId} className="session-tool-detail" hidden={!expanded}>
      {expanded && <>
        <ToolText label={t("sessions.command")} value={details.command} />
        {details.command && details.input ? <details className="session-tool-input">
          <summary>{t("sessions.input_arguments")}</summary><ToolText label={t("sessions.full_input_arguments")} value={details.input} />
        </details> : <ToolText label={t("sessions.input_arguments")} value={details.input} />}
        <ToolText label={t("sessions.working_directory")} value={details.cwd} />
        {(details.exit_code !== undefined || details.duration_ms !== undefined) && <div className="session-tool-metadata">
          {details.exit_code !== undefined && <span>{t("sessions.exit_code")}<code>{details.exit_code}</code></span>}
          {details.duration_ms !== undefined && <span>{t("sessions.duration")}{details.duration_ms < 1 ? '<1' : details.duration_ms} ms</span>}
        </div>}
        <ToolText label={t("sessions.error_output")} value={details.error} error />
        <ToolText label={t("automations.run_output")} value={details.output} />
        {!details.output && !details.error && <p className="session-tool-empty">
          {tool.status === 'in_progress' ? t("sessions.waiting_for_execution_output") : t("sessions.no_output_recorded")}
        </p>}
      </>}
    </div>
  </div>;
}

function ToolGroup({ tools }: { tools: AgentSessionActivity[] }) {
  const [expanded, setExpanded] = useState(false);
  const contentId = useId();
  const summary = toolSummary(tools);
  const status = toolGroupStatus(tools);
  if (tools.length === 1) return <ToolRow tool={tools[0]} />;
  return <div className="session-tool-group">
    <button type="button" className="session-tool-group-toggle" aria-expanded={expanded} aria-controls={contentId}
      aria-label={t("sessions.tool_call", { value0: expanded ? t("aow.collapse") : t("aow.expand"), value1: summary })} onClick={() => setExpanded(!expanded)}>
      <span className="session-tool-group-summary">{summary}</span>
      <span className="session-tool-group-count">{t("acp.call_count", { count: tools.length })}</span>
      <ToolStatus {...status} />
      <ChevronRight className="session-tool-group-chevron" size={13} aria-hidden="true" />
    </button>
    <div id={contentId} hidden={!expanded}>
      {expanded && <ol className="session-tool-group-details">
        {tools.map((tool) => <li key={tool.id}>
          <ActivityTimeMarker activities={[tool]} kind="tool" />
          <ToolRow tool={tool} />
        </li>)}
      </ol>}
    </div>
  </div>;
}

// The parent keys this by session, turn and latest/history role. Snapshot refreshes
// preserve a manual choice; finished and historical turns default to collapsed.
export function SessionTurnProcess({ turn, isLatest }: { turn: AgentSessionSnapshotTurn; isLatest: boolean }) {
  const [expanded, setExpanded] = useState(isLatest && turn.status === 'in_progress');
  useEffect(() => {
    if (turn.status !== 'in_progress') setExpanded(false);
  }, [turn.status]);
  const activities = turn.activities ?? [];
  if (!activities.length && !turn.activities_truncated) return null;
  const toolCount = activities.filter((item) => item.kind === 'tool').length;
  const progressCount = activities.length - toolCount;
  const entries = groupSessionActivities(activities);

  return <ConversationProcess expanded={expanded} onToggle={() => setExpanded(value => !value)}
    summary={[toolCount ? t("acp.tool_calls", { count: toolCount, value0: toolCount }) : '', progressCount ? t("acp.progress_updates", { value0: progressCount }) : ''].filter(Boolean).join(' · ')}>
      {expanded && <>
        {turn.activities_truncated && <p className="session-process-notice">{t("sessions.activity_is_long_showing_the_latest")} {activities.length} {t("sessions.records")}</p>}
        <ol className="session-process-timeline">
          {entries.map((entry) => <li key={entry.id} className={`session-process-item ${entry.kind}`}>
            <ActivityTimeMarker activities={entry.kind === 'tools' ? entry.tools : [entry.item]} kind={entry.kind === 'tools' ? 'tool' : 'commentary'} />
            {entry.kind === 'commentary' ? <p>{entry.item.text}</p> : <ToolGroup tools={entry.tools} />}
          </li>)}
        </ol>
      </>}
  </ConversationProcess>;
}

export function SessionTurnPending({ turn }: { turn: AgentSessionSnapshotTurn }) {
  if (turn.final) return null;
  const descriptions = {
    in_progress: t("sessions.the_agent_is_working_and_has_not_produced_a"),
    failed: t("acp.this_turn_failed_without_a_final_response"),
    interrupted: t("acp.this_turn_was_interrupted_without_a_final_response"),
    completed: t("sessions.no_final_response_recorded_for_this_turn"),
  };
  return <p className={`session-turn-pending ${turn.status}`}>
    {turn.status === 'in_progress' ? <LoaderCircle size={14} className="spinning" aria-hidden="true" /> : <CircleAlert size={14} aria-hidden="true" />}
    {descriptions[turn.status]}
  </p>;
}
