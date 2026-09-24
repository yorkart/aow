import type { AgentSessionActivity, AgentSessionToolAction } from './types';

const recordDateTime = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'medium' });
const recordTime = new Intl.DateTimeFormat('zh-CN', { timeStyle: 'medium' });

export function activityTimeLabel(activities: Pick<AgentSessionActivity, 'timestamp'>[]): string {
  const timestamps = activities.flatMap(({ timestamp }) => {
    const value = timestamp ? Date.parse(timestamp) : NaN;
    return Number.isNaN(value) ? [] : [value];
  });
  if (!timestamps.length) return '时间未记录';
  const first = new Date(Math.min(...timestamps));
  const last = new Date(Math.max(...timestamps));
  let label = recordDateTime.format(first);
  if (label !== recordDateTime.format(last)) {
    const end = first.toDateString() === last.toDateString() ? recordTime.format(last) : recordDateTime.format(last);
    label += ` – ${end}`;
  }
  if (timestamps.length < activities.length) label += '（部分时间未记录）';
  return label;
}

const actionLabels: Record<AgentSessionToolAction, string> = {
  read_files: 'read files', search_files: 'searched content', list_files: 'listed files',
  edit_files: 'edited files', run_commands: 'ran commands', load_tools: 'loaded tools',
  search_web: 'searched the web', update_plan: 'updated the plan', wait: 'waited for results',
  orchestrate: 'executed tools', other: 'called tools',
};
const toolActions: Record<string, AgentSessionToolAction> = {
  read: 'read_files', read_file: 'read_files', read_multiple_files: 'read_files',
  grep: 'search_files', search_files: 'search_files',
  glob: 'list_files', list_files: 'list_files', list_directory: 'list_files',
  apply_patch: 'edit_files', edit: 'edit_files', write: 'edit_files', edit_file: 'edit_files', write_file: 'edit_files',
  exec_command: 'run_commands', bash: 'run_commands', shell: 'run_commands',
  tool_search: 'load_tools', search_tools: 'load_tools', list_tools: 'load_tools', discover_tools: 'load_tools',
  web_search: 'search_web', websearch: 'search_web', webfetch: 'search_web',
  update_plan: 'update_plan', todowrite: 'update_plan',
  exec: 'orchestrate', wait: 'wait', write_stdin: 'wait',
};

function actionsForTool(tool: AgentSessionActivity): AgentSessionToolAction[] {
  const actions = tool.actions?.filter((action) => actionLabels[action]);
  if (actions?.length) return actions;
  const name = tool.text.split(/\.|__/).at(-1)?.toLowerCase() ?? '';
  return [toolActions[name] ?? 'other'];
}

export function toolSummary(tools: AgentSessionActivity[]): string {
  const actions = [...new Set(tools.flatMap(actionsForTool))];
  // Wrapper calls and waits are still counted and available in the expanded list.
  // Prefer the operations they contain when describing a mixed group.
  const concrete = actions.filter((action) => action !== 'orchestrate' && action !== 'wait');
  const summary = (concrete.length ? concrete : actions).map((action) => actionLabels[action]).join(', ');
  return summary.charAt(0).toUpperCase() + summary.slice(1);
}

export type SessionProcessEntry =
  | { kind: 'commentary'; id: string; item: AgentSessionActivity }
  | { kind: 'tools'; id: string; tools: AgentSessionActivity[] };

export function groupSessionActivities(activities: AgentSessionActivity[]): SessionProcessEntry[] {
  const entries: SessionProcessEntry[] = [];
  for (const activity of activities) {
    if (activity.kind === 'commentary') {
      entries.push({ kind: 'commentary', id: activity.id, item: activity });
    } else {
      const previous = entries.at(-1);
      if (previous?.kind === 'tools') previous.tools.push(activity);
      else entries.push({ kind: 'tools', id: activity.id, tools: [activity] });
    }
  }
  return entries;
}

export function toolGroupStatus(tools: AgentSessionActivity[]) {
  const counts = { in_progress: 0, failed: 0, interrupted: 0, unknown: 0, completed: 0 };
  for (const tool of tools) counts[tool.status ?? 'unknown'] += 1;
  const status = counts.in_progress ? 'in_progress' : counts.failed ? 'failed'
    : counts.interrupted ? 'interrupted' : counts.unknown ? 'unknown' : 'completed';
  const label = [
    counts.in_progress ? `${counts.in_progress} 次执行中` : '',
    counts.failed ? `${counts.failed} 次失败` : '',
    counts.interrupted ? `${counts.interrupted} 次中断` : '',
    counts.unknown ? `${counts.unknown} 次结果未记录` : '',
  ].filter(Boolean).join(' · ') || '已完成';
  return { status, label } as const;
}
