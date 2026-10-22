import { t } from '../lib/i18n';
import { Files, FileText, NotebookPen, SquareTerminal } from 'lucide-react';
import type { AowAgent } from '../features/agents/types';
import { AgentIcon } from '../features/agents/AgentIcon';
import { aowAgentType } from '../features/agents/agentTypes';
import { AcpNewSessionMenu } from '../features/acp/workspace';

export function WorkspaceNewMenu({ agents, onOpenFile, onOpenBrowser, onCreateNote, onCreateTerminal, onCreateAcp, onSelect }: {
  agents: AowAgent[];
  onOpenFile?: () => void;
  onOpenBrowser?: () => void;
  onCreateNote: (kind: 'md' | 'txt') => void;
  onCreateTerminal: (agent?: AowAgent) => void;
  onCreateAcp?: (agentId: string) => void;
  onSelect?: () => void;
}) {
  const action = (callback: () => void) => () => { onSelect?.(); callback(); };
  return <div className="project-aow-new-menu">
    <div className="project-aow-menu-label">{t("navigation.workspace")}</div>
    <button data-workspace-open title={t("aow.start_a_shell_in_the_current_worktree")} onClick={action(() => onCreateTerminal())}><SquareTerminal /><span>{t("navigation.terminal")}</span></button>
    <div className="project-aow-menu-label">{t("navigation.agents")}</div>
    {agents.filter(agent => agent.available).map(agent => <button data-workspace-open key={agent.id} title={agent.source === 'detected' ? t("common.auto_detected") : t("im.configured")} onClick={action(() => onCreateTerminal(agent))}><AgentIcon agentId={aowAgentType(agent)} /><span>{agent.display_name}</span></button>)}
    {!agents.some(agent => agent.available) ? <div className="project-aow-menu-empty">{t("aow.no_agents_available")}</div> : null}
    {onCreateAcp ? <AcpNewSessionMenu onCreate={agentId => { onSelect?.(); onCreateAcp(agentId); }} /> : null}
    <div className="project-aow-menu-label">{t("navigation.notes")}</div>
    <button data-workspace-open title={t("aow.create_a_temporary_note_in_the_project_s_notes")} onClick={action(() => onCreateNote('md'))}><NotebookPen /><span>{t("aow.new_markdown")}</span></button>
    <div className="project-aow-menu-label">{t("navigation.files")}</div>
    {onOpenFile ? <button data-workspace-open title={t("aow.open_a_file_using_an_absolute_path_or_a")} onClick={action(onOpenFile)}><FileText /><span>{t("aow.open_file")}</span></button> : null}
    {onOpenBrowser ? <button title={t("aow.browse_and_edit_system_files")} onClick={action(onOpenBrowser)}><Files /><span>{t("aow.open_system_file_browser")}</span></button> : null}
  </div>;
}
