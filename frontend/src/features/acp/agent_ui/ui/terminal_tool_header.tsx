// ui/terminal_tool_header.rs: metadata and controls are separate from the command.
import type { ReactNode } from 'react';
import { ChevronDown, ChevronUp, LoaderCircle, X } from 'lucide-react';

export function ToolDisclosure({ title, open, onToggle }: { title: string; open: boolean; onToggle: () => void }) {
  return <button type="button" className="acp-quiet-button acp-tool-disclosure" aria-label={title} title={open ? '收起详情' : '展开详情'} aria-expanded={open} onClick={onToggle}>{open ? <ChevronUp size={13} /> : <ChevronDown size={13} />}</button>;
}

export function TerminalToolHeader({ title, directory, open, running, failure, onToggle, children }: { title: string; directory?: string; open: boolean; running: boolean; failure?: string; onToggle?: () => void; children: ReactNode }) {
  return <div className="acp-terminal-header">
    <div className="acp-terminal-metadata">
      <span className="acp-terminal-directory" title={directory}><bdi>{directory || 'Run Command'}</bdi></span>
      {onToggle && <ToolDisclosure title={title} open={open} onToggle={onToggle} />}
      {running && <LoaderCircle size={13} className="acp-spinner" aria-label="执行中" />}
      {failure && <span title={failure}><X size={13} className="acp-tool-error-icon" aria-label={failure} /></span>}
    </div>
    {children}
  </div>;
}
