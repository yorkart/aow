import { t } from '../../../../lib/i18n';
import type { ReactNode } from 'react';
import { ChevronDown, ChevronUp, LoaderCircle, X } from 'lucide-react';

export function ToolDisclosure({ title, open, onToggle }: { title: string; open: boolean; onToggle: () => void }) {
  return <button type="button" className="acp-quiet-button acp-tool-disclosure" aria-label={title} title={open ? t("acp.collapse_details") : t("acp.expand_details")} aria-expanded={open} onClick={onToggle}>{open ? <ChevronUp size={13} /> : <ChevronDown size={13} />}</button>;
}

export function TerminalToolHeader({ title, directory, open, running, failure, onToggle, children }: { title: string; directory?: string; open: boolean; running: boolean; failure?: string; onToggle?: () => void; children: ReactNode }) {
  return <div className="acp-terminal-header">
    <div className="acp-terminal-metadata">
      <span className="acp-terminal-directory" title={directory}><bdi>{directory || t("acp.run_command")}</bdi></span>
      {onToggle && <ToolDisclosure title={title} open={open} onToggle={onToggle} />}
      {running && <LoaderCircle size={13} className="acp-spinner" aria-label={t("aow.running")} />}
      {failure && <span title={failure}><X size={13} className="acp-tool-error-icon" aria-label={failure} /></span>}
    </div>
    {children}
  </div>;
}
