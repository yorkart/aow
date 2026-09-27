import { Files, FileText, NotebookPen, SquareTerminal } from 'lucide-react';
import type { AowAgent } from '../features/agents/types';
import { AgentIcon } from '../features/agents/AgentIcon';
import { aowAgentType } from '../features/agents/agentTypes';

export function WorkspaceNewMenu({ agents, onOpenFile, onOpenBrowser, onCreateNote, onCreateTerminal, onSelect }: {
  agents: AowAgent[];
  onOpenFile?: () => void;
  onOpenBrowser?: () => void;
  onCreateNote: (kind: 'md' | 'txt') => void;
  onCreateTerminal: (agent?: AowAgent) => void;
  onSelect?: () => void;
}) {
  const action = (callback: () => void) => () => { onSelect?.(); callback(); };
  return <div className="project-aow-new-menu">
    <div className="project-aow-menu-label">Files</div>
    {onOpenFile ? <button data-workspace-open title="输入绝对路径或当前 Worktree 的相对路径打开文件" onClick={action(onOpenFile)}><FileText /><span>打开文件</span></button> : null}
    {onOpenBrowser ? <button title="浏览系统文件并打开编辑" onClick={action(onOpenBrowser)}><Files /><span>打开系统文件浏览器</span></button> : null}
    <div className="project-aow-menu-label">Notes</div>
    <button data-workspace-open title="在 Project Notes 根目录创建临时笔记" onClick={action(() => onCreateNote('md'))}><NotebookPen /><span>新建 Markdown</span></button>
    <button data-workspace-open title="在 Project Notes 根目录创建临时文本" onClick={action(() => onCreateNote('txt'))}><FileText /><span>新建 TXT 文件</span></button>
    <div className="project-aow-menu-label">Workspace</div>
    <button data-workspace-open title="在当前 Worktree 启动 shell" onClick={action(() => onCreateTerminal())}><SquareTerminal /><span>Terminal</span></button>
    <div className="project-aow-menu-label">Agents</div>
    {agents.filter(agent => agent.available).map(agent => <button data-workspace-open key={agent.id} title={agent.source === 'detected' ? 'Auto detected' : 'Configured'} onClick={action(() => onCreateTerminal(agent))}><AgentIcon agentId={aowAgentType(agent)} /><span>{agent.display_name}</span></button>)}
    {!agents.some(agent => agent.available) ? <div className="project-aow-menu-empty">未发现可用 Agent</div> : null}
  </div>;
}
