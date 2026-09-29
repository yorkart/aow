import { useEffect, useState } from 'react';
import { InboxPanel } from './InboxPanel';
import { TaskBoard } from './TaskBoard';
import { ConvertDialog } from './ConvertDialog';
import { useTaskBoard, taskError } from './api';
import { agentsApi } from '../agents/api';
import type { AowAgent } from '../agents/types';
import type { AowProject, AowWorktree } from '../../aow/types';
import type { MobileNavigate } from '../../mobile/mobileState';
import { terminalApi } from '../terminals/terminalApi';
import type { InboxItem } from './types';

export function MobileTasks({ project, worktree, visible, navigate }: { project: AowProject; worktree: AowWorktree; visible: boolean; navigate: MobileNavigate }) {
  const state = useTaskBoard(project.id, visible);
  const [view, setView] = useState<'inbox' | 'board'>('inbox');
  const [item, setItem] = useState<InboxItem>();
  const [agents, setAgents] = useState<AowAgent[]>([]);
  const [error, setError] = useState('');
  useEffect(() => { void agentsApi.agents().then(setAgents).catch(e => setError(taskError(e))); }, []);
  return <div className="tasks-mobile"><nav><button aria-pressed={view === 'inbox'} onClick={() => setView('inbox')}>Inbox</button><button aria-pressed={view === 'board'} onClick={() => setView('board')}>Task Board</button></nav>
    {error && <p className="tasks-error" role="alert">{error}</p>}
    {view === 'inbox' ? <InboxPanel state={state} onConvert={setItem} onTask={() => setView('board')} /> : <TaskBoard state={state} onOpen={async task => {
      if (!task.tab_id) return; const terminal = await terminalApi.get(task.tab_id); navigate({ workspace: terminal.workspace_root, view: 'terminal', terminal: terminal.id });
    }} />}
    {item && state.board && <ConvertDialog item={item} agents={agents} statuses={state.board.statuses} project={project} worktree={worktree} onClose={() => setItem(undefined)} onCreated={() => setView('board')} />}
  </div>;
}
