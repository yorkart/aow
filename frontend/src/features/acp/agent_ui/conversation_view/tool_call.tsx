// thread_view.rs::render_tool_call, render_terminal_tool_call and render_tool_call_label.
import { memo, useState } from 'react';
import { ArrowRightLeft, Brain, ChevronUp, FilePenLine, Globe, Search, Terminal, Trash2, Wrench, X } from 'lucide-react';
import { list, object, text, type Data, type Permission } from '../../types';
import { Command } from '../ui/command';
import { TerminalToolHeader, ToolDisclosure } from '../ui/terminal_tool_header';
import { ContentBlock, DiffContent, type OpenFile } from './content';
import { PermissionRequest } from './elicitation';
import { AgentMarkdown } from './markdown';
import { rawToolMarkdown, toolContent } from './entry_view_state';
import { TerminalOutput } from './terminal_output';

const icons = { read: Search, search: Search, edit: FilePenLine, delete: Trash2, move: ArrowRightLeft, execute: Terminal, think: Brain, fetch: Globe, switch_mode: ArrowRightLeft, other: Wrench };
const locationLabel = (location: Data) => `${text(location.path)}${typeof location.line === 'number' ? `:${location.line}` : ''}`;

export const ToolEntry = memo(function ToolEntry({ content, terminals, onOpenFile, permission, busy, onAction }: {
  content: Data; terminals?: Record<string, Data>; onOpenFile: OpenFile; permission?: Permission; busy: boolean;
  onAction: (action: Data) => Promise<boolean>;
}) {
  return <ToolCall content={content} terminals={terminals} onOpenFile={onOpenFile} permission={permission} busy={busy}
    onAnswer={response => { if (permission) void onAction({ action: 'answer', request_id: permission.id, response }); }} />;
}, (previous, next) => {
  if (previous.content !== next.content || previous.permission !== next.permission || previous.busy !== next.busy
    || previous.onOpenFile !== next.onOpenFile || previous.onAction !== next.onAction) return false;
  return previous.terminals === next.terminals || toolContent(next.content).filter(part => part.type === 'terminal')
    .every(part => previous.terminals?.[text(part.terminalId)] === next.terminals?.[text(part.terminalId)]);
});

function ToolLabel({ content, title, edit, onOpenFile }: { content: Data; title: string; edit: boolean; onOpenFile: OpenFile }) {
  const locations = list(content.locations);
  const location = locations.length === 1 ? locations[0] : undefined;
  const Icon = icons[text(content.kind) as keyof typeof icons] || Wrench;
  const label = edit ? title : title.split('\n')[0] + (title.includes('\n') ? '…' : '');
  const open = () => { if (location) onOpenFile(text(location.path), typeof location.line === 'number' ? location.line : undefined); };
  return <div className={`acp-tool-label${location ? ' acp-tool-location' : ''}`} role={location ? 'button' : undefined} tabIndex={location ? 0 : undefined} aria-label={location ? locationLabel(location) : undefined} title={location ? `打开文件：${locationLabel(location)}` : title} onClick={event => {
    if (!(event.target instanceof Element) || !event.target.closest('a')) open();
  }} onKeyDown={event => {
    if (event.target === event.currentTarget && location && ['Enter', ' '].includes(event.key)) { event.preventDefault(); open(); }
  }}>
    <Icon size={14} aria-hidden />
    <div className="acp-tool-label-text">{edit || !text(content.title).trim() ? <span>{label}</span> : <AgentMarkdown text={label} controls={false} onOpenFile={onOpenFile} />}</div>
  </div>;
}

export function ToolCall({ content, terminals, onOpenFile, permission, busy = false, onAnswer }: { content: Data; terminals?: Record<string, Data>; onOpenFile: OpenFile; permission?: Permission; busy?: boolean; onAnswer?: (response: Data) => void }) {
  const [expanded, setExpanded] = useState(false);
  const [rawExpanded, setRawExpanded] = useState(false);
  const parts = toolContent(content);
  const terminalParts = parts.filter(part => part.type === 'terminal');
  const edit = content.kind === 'edit' || parts.some(part => part.type === 'diff');
  const execute = content.kind === 'execute';
  const showRawInput = !edit && !execute && !parts.some(part => object(part.content).type === 'image');
  const open = expanded || !!permission;
  const pending = ['pending', 'in_progress'].includes(text(content.status));
  const failed = ['failed', 'rejected', 'cancelled', 'canceled'].includes(text(content.status));
  const rejected = content.status === 'rejected' && !permission;
  const card = edit || execute || !!permission;
  const title = text(content.title).trim() ? text(content.title) : text(content.toolName) || 'Tool call';
  const hasContent = parts.length > 0 || showRawInput && content.rawInput != null;
  const toggle = () => setExpanded(value => !value);
  const authorization = permission && onAnswer && <PermissionRequest permission={permission} busy={busy} onAnswer={onAnswer} embedded />;
  // Terminal-bearing calls use their own cards even when the tool kind is absent.
  if (terminalParts.length) return <section className="acp-tool-terminals" aria-label={title}>
    {terminalParts.map((part, index) => {
      const terminal = terminals?.[text(part.terminalId)];
      const exit = object(terminal?.exit_status);
      const exitFailed = typeof exit.exitCode === 'number' && exit.exitCode !== 0 || !!exit.signal;
      const failure = exitFailed ? typeof exit.exitCode === 'number' ? `退出码：${exit.exitCode}` : text(exit.signal) : failed ? '执行失败或已取消' : undefined;
      return <div key={text(part.terminalId) || index} className={`acp-tool acp-tool-card${failure ? ' acp-tool-failed' : ''}`}>
        <TerminalToolHeader title={title} directory={text(terminal?.cwd) || 'current directory'} open={expanded} running={pending && !permission && terminal?.exit_status == null} failure={failure} onToggle={toggle}><Command title={title} /></TerminalToolHeader>
        {expanded && <div className="acp-tool-output acp-terminal-body"><TerminalOutput terminal={terminal} pending={pending} /></div>}
        {authorization}
      </div>;
    })}
  </section>;

  const output = parts.map((part, index) => <div className="acp-tool-content" key={index}>
    {part.type === 'content' ? <>
      {object(part.content).type === 'image' && list(content.locations)[0] && <button type="button" className="acp-file-link" onClick={() => { const location = list(content.locations)[0]; onOpenFile(text(location.path), typeof location.line === 'number' ? location.line : undefined); }}>打开文件</button>}
      <ContentBlock content={object(part.content)} onOpenFile={onOpenFile} />
    </> : part.type === 'diff' ? <DiffContent content={part} onOpenFile={onOpenFile} /> : null}
  </div>);
  return <section className={`acp-tool${card ? ' acp-tool-card' : ''}${failed ? ' acp-tool-failed' : ''}`} aria-label={title}>
    {execute ? <TerminalToolHeader title={title} open={open} running={pending && !permission} failure={failed ? '执行失败或已取消' : undefined} onToggle={hasContent && !permission ? toggle : undefined}><Command title={title} /></TerminalToolHeader>
      : <div className="acp-tool-header">
        <ToolLabel content={content} title={title} edit={edit} onOpenFile={onOpenFile} />
        {hasContent && !permission && <ToolDisclosure title={title} open={open} onToggle={toggle} />}
        {permission && <span className="acp-tool-approval-state" role="status">等待授权</span>}
        {failed && <X size={13} className="acp-tool-error-icon" aria-label="失败或已取消" />}
      </div>}
    {open && !rejected && <div className="acp-tool-output">
      {!permission && showRawInput && <div className="acp-tool-input"><div className="acp-tool-section-label">Raw Input:</div><AgentMarkdown text={rawToolMarkdown(content.rawInput)} onOpenFile={onOpenFile} /><div className="acp-tool-section-label">Output:</div></div>}
      {output}
      {permission && showRawInput && <div className="acp-tool-input acp-tool-confirm-input"><button type="button" className="acp-tool-input-toggle" aria-expanded={rawExpanded} onClick={() => setRawExpanded(value => !value)}><span>{rawExpanded ? 'Raw Input:' : 'View Raw Input'}</span><ChevronUp size={12} style={{ transform: rawExpanded ? undefined : 'rotate(180deg)' }} /></button>{rawExpanded && <AgentMarkdown text={rawToolMarkdown(content.rawInput)} onOpenFile={onOpenFile} />}</div>}
      {!card && <button type="button" className="acp-tool-collapse" aria-label="收起工具详情" onClick={toggle}><ChevronUp size={13} /></button>}
    </div>}
    {authorization}
  </section>;
}
