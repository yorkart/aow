import { t } from '../../lib/i18n';
import { useId, useState, type ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import type { AowWorktree } from '../../aow/types';
import { useAowTabNavigation } from '../../aow/tabNavigation';
import { AgentIcon } from '../agents/AgentIcon';
import { AcpIcon } from '../agents/AcpIcon';
import { instanceKey, type InstanceInfo, type InstanceRef } from '../instances/types';
import { terminalPaneAgent, terminalPaneTitle, terminalTabPresentation } from './terminalPresentation';
import type { TerminalPane, TerminalTab } from './types';
import { TaskCompletion } from '../../components/TaskCompletion';
import { requestTerminalPaneSelection } from './terminalViewState';
import '../instances/instance-family.css';

interface Entry { info: InstanceInfo; tab?: TerminalTab; pane?: TerminalPane }
export type TerminalChildren = Map<string, Entry[]>;

export function indexTerminalChildren(tabs: TerminalTab[], instances: InstanceInfo[] = []): TerminalChildren {
  const entries = new Map<string, Entry>(instances.map(info => [instanceKey(info), { info }]));
  for (const tab of tabs) for (const pane of tab.panes) {
    const info: InstanceInfo = { kind: 'terminal', id: pane.id, parent: pane.parent ?? null,
      title: tab.name, cwd: tab.workspace_root, agent_id: pane.agent_id ?? null, tab_id: tab.id, url: `/aow/tabs/terminal/${tab.id}`, deleted: false };
    entries.set(instanceKey(info), { info, tab, pane });
  }
  const children: TerminalChildren = new Map();
  for (const entry of entries.values()) {
    const { info } = entry;
    if (!info.parent || instanceKey(info.parent) === instanceKey(info)) continue;
    const key = instanceKey(info.parent);
    const siblings = children.get(key) ?? [];
    siblings.push(entry); children.set(key, siblings);
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
  onOpen?: (tab: TerminalTab, paneId: string) => void;
}

function descendants(ids: string[], ancestors: Set<string>, index: TerminalChildren) {
  return ids.flatMap(id => index.get(id) ?? []).filter(entry => !ancestors.has(instanceKey(entry.info)));
}

function Descendant({ entry, ancestors, ...props }: Props & { entry: Entry; ancestors: Set<string> }) {
  const { tab, pane, info } = entry;
  const navigate = useAowTabNavigation();
  const [error, setError] = useState('');
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  const key = instanceKey(info);
  const visited = new Set([...ancestors, key]);
  const children = descendants([key], visited, props.index);
  const worktree = props.worktrees?.find(worktree => worktree.path === info.cwd);
  const branch = worktree?.branch || (worktree?.detached ? t("git.detached_head") : info.cwd.split('/').filter(Boolean).at(-1)) || info.cwd;
  const title = tab && pane ? (tab.panes.length === 1 ? terminalTabPresentation(tab, props.detectedAgents, props.titles).title
    : terminalPaneTitle(pane, props.detectedAgents, props.titles)) : info.title;
  const label = `${branch ? `${branch} · ` : ''}${info.kind === 'acp' ? 'ACP · ' : ''}${title}${info.deleted ? t("common.deleted") : ''}`;
  const open = async () => {
    setError('');
    if (tab && pane && props.onOpen) { props.onOpen(tab, pane.id); return; }
    try {
      if (info.kind === 'acp') await navigate({ type: 'acp', thread_id: info.id });
      else if (info.tab_id) {
        requestTerminalPaneSelection(info.tab_id, info.id);
        await navigate({ type: 'terminal', tabId: info.tab_id });
      }
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
  };
  return <li className="terminal-descendant" data-instance={key} data-pane-id={info.kind === 'terminal' ? info.id : undefined}>
    <div className={`terminal-descendant-row${tab?.id === props.activeId && props.activeId ? ' active' : ''}`}>
      {children.length ? <button type="button" className="terminal-children-toggle" aria-label={t("terminals.child_instances_of", { value0: expanded ? t("aow.collapse") : t("aow.expand"), value1: label })}
        aria-expanded={expanded} aria-controls={bodyId} onClick={() => setExpanded(value => !value)}>
        <ChevronRight className={expanded ? 'expanded' : undefined} />
      </button> : <span className="terminal-children-spacer" />}
      <button type="button" className="terminal-descendant-open" title={`${label}\n${info.cwd}\n${key}`} disabled={info.deleted}
        onClick={() => void open()}>
        {info.kind === 'acp' ? <AcpIcon /> : <AgentIcon agentId={pane ? terminalPaneAgent(pane, props.detectedAgents) ?? pane.agent_id : info.agent_id} />}<span>{label}</span>
        {tab && props.unreadTerminalIds.has(tab.id) && <TaskCompletion />}
      </button>
    </div>
    {error && <p className="terminal-panel-error" role="alert">{error}</p>}
    {children.length ? <ul id={bodyId} className="terminal-descendants" hidden={!expanded}>
      {expanded ? children.map(child => <Descendant key={instanceKey(child.info)} {...props} entry={child} ancestors={visited} />) : null}
    </ul> : null}
  </li>;
}

export function InstanceDescendants({ roots, title, tabId, children: row, ...props }: Props & { roots: InstanceRef[]; title: string; tabId?: string; children: ReactNode }) {
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  const ids = roots.map(instanceKey);
  const ancestors = new Set(ids);
  const children = descendants(ids, ancestors, props.index);
  return <div className="terminal-family" data-tab-id={tabId} data-instance={ids[0]}>
    <div className={`terminal-family-root${children.length ? ' has-children' : ''}`}>
      {children.length ? <button type="button" className="terminal-children-toggle" aria-label={t("terminals.child_instances_of", { value0: expanded ? t("aow.collapse") : t("aow.expand"), value1: title })}
        aria-expanded={expanded} aria-controls={bodyId} onClick={() => setExpanded(value => !value)}>
        <ChevronRight className={expanded ? 'expanded' : undefined} />
      </button> : null}
      {row}
    </div>
    {children.length ? <ul id={bodyId} className="terminal-descendants" hidden={!expanded}>
      {expanded ? children.map(child => <Descendant key={instanceKey(child.info)} {...props} entry={child} ancestors={ancestors} />) : null}
    </ul> : null}
  </div>;
}

// Preserve independent expansion in each existing terminal row.
export function TerminalDescendants({ tab, ...props }: Props & { tab: TerminalTab; title: string; children: ReactNode }) {
  return <InstanceDescendants {...props} tabId={tab.id} roots={tab.panes.map(pane => ({ kind: 'terminal', id: pane.id }))} />;
}
