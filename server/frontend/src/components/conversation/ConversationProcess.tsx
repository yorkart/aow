import { t } from '../../lib/i18n';
import { useEffect, useId, useState, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import './conversation-process.css';

// Refreshes preserve manual choices; a turn lifecycle change resets all disclosures.
export function useProcessExpansion(running: boolean, required = false) {
  const [expanded, setExpanded] = useState(running);
  useEffect(() => { setExpanded(running); }, [running]);
  return { expanded: required || expanded, toggle: () => setExpanded(value => !value) };
}

export function ConversationProcess({ expanded, onToggle, summary, required = false, children }: {
  expanded: boolean; onToggle: () => void; summary: ReactNode; required?: boolean; children: ReactNode;
}) {
  const contentId = useId();
  return <div className={`session-turn-process${expanded ? ' expanded' : ''}`}>
    <button type="button" className="session-process-toggle" aria-expanded={expanded} aria-controls={contentId}
      disabled={required} aria-label={expanded ? t("components.collapse_activity") : t("components.expand_activity")} onClick={onToggle}>
      <ChevronRight size={14} className="session-process-chevron" aria-hidden="true" />
      <span>{t("components.activity")}</span><span className="session-process-count">{summary}</span>
      <span className="session-process-action">{required ? t("components.waiting_for_action") : expanded ? t("aow.collapse") : t("aow.expand")}</span>
    </button>
    <div id={contentId} hidden={!expanded}>{children}</div>
  </div>;
}
