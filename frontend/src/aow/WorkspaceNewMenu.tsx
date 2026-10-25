import { t } from '../lib/i18n';
import { useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { FileText, NotebookPen, SquareTerminal } from 'lucide-react';
import type { AowAgent } from '../features/agents/types';
import { AgentIcon } from '../features/agents/AgentIcon';
import { aowAgentType } from '../features/agents/agentTypes';
import { AcpNewSessionMenu } from '../features/acp/workspace';

export function WorkspaceNewMenu({ anchor, agents, onOpenFile, onCreateNote, onCreateTerminal, onCreateAcp, onSelect }: {
  anchor: RefObject<HTMLElement | null>;
  agents: AowAgent[];
  onOpenFile?: () => void;
  onCreateNote: (kind: 'md' | 'txt') => void;
  onCreateTerminal: (agent?: AowAgent) => void;
  onCreateAcp?: (agentId: string) => void;
  onSelect?: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0, maxHeight: window.innerHeight - 16 });
  useLayoutEffect(() => {
    const element = menu.current;
    if (!element) return;
    const update = () => {
      const bounds = anchor.current?.getBoundingClientRect();
      if (!bounds) return;
      const below = Math.max(0, window.innerHeight - bounds.bottom - 12);
      const above = Math.max(0, bounds.top - 12);
      const height = element.scrollHeight + 2;
      const upwards = height > below && above > below;
      const maxHeight = upwards ? above : below;
      const next = {
        left: Math.max(8, Math.min(bounds.left, window.innerWidth - element.offsetWidth - 8)),
        top: upwards ? bounds.top - Math.min(height, maxHeight) - 4 : bounds.bottom + 4,
        maxHeight,
      };
      setPosition(previous => previous.left === next.left && previous.top === next.top && previous.maxHeight === next.maxHeight ? previous : next);
    };
    update();
    // ACP agents load asynchronously and can change the menu's height.
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [anchor]);
  const action = (callback: () => void) => () => { onSelect?.(); callback(); };
  return <div ref={menu} className="project-aow-new-menu" style={position}>
    <div className="project-aow-menu-label">{t("navigation.workspace")}</div>
    <button data-workspace-open title={t("aow.start_a_shell_in_the_current_worktree")} onClick={action(() => onCreateTerminal())}><SquareTerminal /><span>{t("navigation.terminal")}</span></button>
    <div className="project-aow-menu-label">{t("navigation.agents")}</div>
    {agents.filter(agent => agent.available).map(agent => <button data-workspace-open key={agent.id} title={agent.source === 'detected' ? t("common.auto_detected") : t("im.configured")} onClick={action(() => onCreateTerminal(agent))}><AgentIcon agentId={aowAgentType(agent)} /><span>{agent.display_name}</span></button>)}
    {!agents.some(agent => agent.available) ? <div className="project-aow-menu-empty">{t("aow.no_agents_available")}</div> : null}
    {onCreateAcp ? <AcpNewSessionMenu onCreate={agentId => { onSelect?.(); onCreateAcp(agentId); }} /> : null}
    <div className="project-aow-menu-label">{t("navigation.notes")}</div>
    <button data-workspace-open title={t("aow.create_a_temporary_note_in_the_project_s_notes")} onClick={action(() => onCreateNote('md'))}><NotebookPen /><span>{t("aow.new_markdown")}</span></button>
    {onOpenFile ? <div className="project-aow-menu-label">{t("navigation.files")}</div> : null}
    {onOpenFile ? <button data-workspace-open title={t("aow.open_a_file_using_an_absolute_path_or_a")} onClick={action(onOpenFile)}><FileText /><span>{t("aow.open_file")}</span></button> : null}
  </div>;
}
