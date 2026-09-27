import { useEffect, useId, useState } from 'react';
import { Check, ChevronRight, Circle, CircleAlert, LoaderCircle, MessageSquare, Minus, Wrench } from 'lucide-react';
import type { AgentSessionActivity, AgentSessionSnapshotTurn, AgentSessionToolText } from './types';
import { activityTimeLabel, groupSessionActivities, toolGroupStatus, toolSummary } from './sessionProcessPresentation';
import './session-turn-process.css';

const statusLabels = { completed: '已完成', failed: '失败', interrupted: '已中断', in_progress: '执行中', unknown: '结果未记录' };

function ActivityTimeMarker({ activities, kind }: { activities: AgentSessionActivity[]; kind: AgentSessionActivity['kind'] }) {
  const label = activityTimeLabel(activities);
  const description = kind === 'commentary' ? '进度说明时间' : activities.length > 1 ? '工具组记录时间' : '工具调用时间';
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
    <div className="session-tool-text-label">{label}{value.truncated && <span>内容过长，仅保留开头和结尾</span>}</div>
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
      aria-label={`${expanded ? '收起' : '展开'}工具调用详情：${tool.text}`} onClick={() => setExpanded(!expanded)}>
      {heading}
      <ChevronRight className="session-tool-chevron" size={13} aria-hidden="true" />
    </button>
    {preview && <code className="session-tool-preview">{preview}</code>}
    <div id={contentId} className="session-tool-detail" hidden={!expanded}>
      {expanded && <>
        <ToolText label="命令" value={details.command} />
        {details.command && details.input ? <details className="session-tool-input">
          <summary>输入参数</summary><ToolText label="完整输入参数" value={details.input} />
        </details> : <ToolText label="输入参数" value={details.input} />}
        <ToolText label="工作目录" value={details.cwd} />
        {(details.exit_code !== undefined || details.duration_ms !== undefined) && <div className="session-tool-metadata">
          {details.exit_code !== undefined && <span>退出码：<code>{details.exit_code}</code></span>}
          {details.duration_ms !== undefined && <span>耗时：{details.duration_ms < 1 ? '<1' : details.duration_ms} ms</span>}
        </div>}
        <ToolText label="错误输出" value={details.error} error />
        <ToolText label="执行输出" value={details.output} />
        {!details.output && !details.error && <p className="session-tool-empty">
          {tool.status === 'in_progress' ? '等待执行输出…' : '未记录输出内容。'}
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
      aria-label={`${expanded ? '收起' : '展开'}工具调用：${summary}`} onClick={() => setExpanded(!expanded)}>
      <span className="session-tool-group-summary">{summary}</span>
      <span className="session-tool-group-count">{tools.length} 次调用</span>
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
  const contentId = useId();
  useEffect(() => {
    if (turn.status !== 'in_progress') setExpanded(false);
  }, [turn.status]);
  const activities = turn.activities ?? [];
  if (!activities.length && !turn.activities_truncated) return null;
  const toolCount = activities.filter((item) => item.kind === 'tool').length;
  const progressCount = activities.length - toolCount;
  const entries = groupSessionActivities(activities);

  return <div className={`session-turn-process${expanded ? ' expanded' : ''}`}>
    <button type="button" className="session-process-toggle" aria-expanded={expanded} aria-controls={contentId}
      aria-label={expanded ? '收起处理过程' : '展开处理过程'} onClick={() => setExpanded((value) => !value)}>
      <ChevronRight size={14} className="session-process-chevron" aria-hidden="true" />
      <span>处理过程</span>
      <span className="session-process-count">{[toolCount ? `${toolCount} 次工具调用` : '', progressCount ? `${progressCount} 条进度` : ''].filter(Boolean).join(' · ')}</span>
      <span className="session-process-action">{expanded ? '收起' : '展开'}</span>
    </button>
    <div id={contentId} hidden={!expanded}>
      {expanded && <>
        {turn.activities_truncated && <p className="session-process-notice">过程较长，仅展示最近的 {activities.length} 条记录。</p>}
        <ol className="session-process-timeline">
          {entries.map((entry) => <li key={entry.id} className={`session-process-item ${entry.kind}`}>
            <ActivityTimeMarker activities={entry.kind === 'tools' ? entry.tools : [entry.item]} kind={entry.kind === 'tools' ? 'tool' : 'commentary'} />
            {entry.kind === 'commentary' ? <p>{entry.item.text}</p> : <ToolGroup tools={entry.tools} />}
          </li>)}
        </ol>
      </>}
    </div>
  </div>;
}

export function SessionTurnPending({ turn }: { turn: AgentSessionSnapshotTurn }) {
  if (turn.final) return null;
  const descriptions = {
    in_progress: 'Agent 正在处理，尚未生成最终结论。',
    failed: '本轮执行失败，未生成最终结论。',
    interrupted: '本轮已中断，未生成最终结论。',
    completed: '本轮未记录最终结论。',
  };
  return <p className={`session-turn-pending ${turn.status}`}>
    {turn.status === 'in_progress' ? <LoaderCircle size={14} className="spinning" aria-hidden="true" /> : <CircleAlert size={14} aria-hidden="true" />}
    {descriptions[turn.status]}
  </p>;
}
