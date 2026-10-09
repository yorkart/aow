import { useId, useState, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import type { AowWorktree } from '../../aow/types';
import { AgentIcon } from '../agents/AgentIcon';
import { terminalPaneAgent, terminalPaneTitle, terminalTabPresentation } from './terminalPresentation';
import type { TerminalPane, TerminalTab } from './types';
import { TerminalTaskCompletion } from './TerminalTaskCompletion';

interface Entry { tab: TerminalTab; pane: TerminalPane }
export type TerminalChildren = Map<string, Entry[]>;

export function indexTerminalChildren(tabs: TerminalTab[]): TerminalChildren {
  const children: TerminalChildren = new Map();
  for (const tab of tabs) for (const pane of tab.panes) {
    if (!pane.parent_pane_id || pane.parent_pane_id === pane.id) continue;
    const siblings = children.get(pane.parent_pane_id) ?? [];
    siblings.push({ tab, pane });
    children.set(pane.parent_pane_id, siblings);
  }
  return children;
}

interface Props {
  index: TerminalChildren;
  worktrees?: AowWorktree[];
  detectedAgents: Record<string, string | null>;
  titles: Record<string, string>;
  activeId?: string;
  unreadTerminalIds: ReadonlySet<string>;
  onOpen: (tab: TerminalTab, paneId: string) => void;
}

function descendants(ids: string[], ancestors: Set<string>, index: TerminalChildren) {
  return ids.flatMap(id => index.get(id) ?? []).filter(entry => !ancestors.has(entry.pane.id));
}

function Descendant({ entry, ancestors, ...props }: Props & { entry: Entry; ancestors: Set<string> }) {
  const { tab, pane } = entry;
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  const visited = new Set([...ancestors, pane.id]);
  const children = descendants([pane.id], visited, props.index);
  const worktree = props.worktrees?.find(worktree => worktree.path === tab.workspace_root);
  const branch = worktree?.branch || (worktree?.detached ? 'detached' : tab.workspace_root.split('/').filter(Boolean).at(-1)) || tab.workspace_root;
  const title = tab.panes.length === 1 ? terminalTabPresentation(tab, props.detectedAgents, props.titles).title
    : terminalPaneTitle(pane, props.detectedAgents, props.titles);
  const label = `${branch} · ${title}`;
  return <li className="terminal-descendant" data-pane-id={pane.id}>
    <div className={`terminal-descendant-row${tab.id === props.activeId ? ' active' : ''}`}>
      {children.length ? <button type="button" className="terminal-children-toggle" aria-label={`${expanded ? '收起' : '展开'} ${label} 的子实例`}
        aria-expanded={expanded} aria-controls={bodyId} onClick={() => setExpanded(value => !value)}>
        <ChevronRight className={expanded ? 'expanded' : undefined} />
      </button> : <span className="terminal-children-spacer" />}
      <button type="button" className="terminal-descendant-open" title={`${label}\n${tab.workspace_root}`}
        onClick={() => props.onOpen(tab, pane.id)}>
        <AgentIcon agentId={terminalPaneAgent(pane, props.detectedAgents) ?? pane.agent_id} /><span>{label}</span>
        {props.unreadTerminalIds.has(tab.id) && <TerminalTaskCompletion />}
      </button>
    </div>
    {children.length ? <ul id={bodyId} className="terminal-descendants" hidden={!expanded}>
      {expanded ? children.map(child => <Descendant key={child.pane.id} {...props} entry={child} ancestors={visited} />) : null}
    </ul> : null}
  </li>;
}

// Each existing list row owns its expansion state. The same pane can therefore
// appear independently in its own row and in any ancestor's descendants.
export function TerminalDescendants({ tab, title, children: row, ...props }: Props & { tab: TerminalTab; title: string; children: ReactNode }) {
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  const ids = tab.panes.map(pane => pane.id);
  const ancestors = new Set(ids);
  const children = descendants(ids, ancestors, props.index);
  return <div className="terminal-family" data-tab-id={tab.id}>
    <div className={`terminal-family-root${children.length ? ' has-children' : ''}`}>
      {children.length ? <button type="button" className="terminal-children-toggle" aria-label={`${expanded ? '收起' : '展开'} ${title} 的子实例`}
        aria-expanded={expanded} aria-controls={bodyId} onClick={() => setExpanded(value => !value)}>
        <ChevronRight className={expanded ? 'expanded' : undefined} />
      </button> : null}
      {row}
    </div>
    {children.length ? <ul id={bodyId} className="terminal-descendants" hidden={!expanded}>
      {expanded ? children.map(child => <Descendant key={child.pane.id} {...props} entry={child} ancestors={ancestors} />) : null}
    </ul> : null}
  </div>;
}
