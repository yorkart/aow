import { createRoot } from 'react-dom/client';
import { useState } from 'react';
import { ConversationPanel } from '../src/features/conversations/ConversationPanel';
import { useInstanceGraph } from '../src/features/instances/useInstanceGraph';
import { TaskScopeProvider } from '../src/features/tasks/TaskScope';
import type { WorkTask } from '../src/features/tasks/api';
import '../src/styles.css';

const task: WorkTask = { status: 'todo', created_at: null, updated_at: null, completed_at: null, id: 'task', project_id: 'project', workspace: '/repo', prompt: 'Task', agent_id: 'codex',
  workspace_mode: 'existing', slug: null, initial_thread_id: 'root', pinned: false, archived: false,
  resources: [{ kind: 'acp', id: 'root' }], dependencies: ['other'] };
const other: WorkTask = { ...task, id: 'other', resources: [{ kind: 'acp', id: 'other' }], dependencies: [] };
function Preview() {
  const graph = useInstanceGraph();
  const [mode, setMode] = useState('worktree');
  const [opened, setOpened] = useState('');
  const panel = <ConversationPanel workspace="/repo" visible onOpenAcp={session => setOpened(`acp:${session.thread_id}`)} onOpenTerminal={tab => setOpened(`terminal:${tab.id}`)} />;
  return <><button onClick={() => setMode(mode === 'task' ? 'worktree' : 'task')}>{mode}</button><output aria-label="Opened instance">{opened}</output>
    <aside style={{ width: 320, height: 600 }}>{mode === 'task' ? <TaskScopeProvider task={task} tasks={[task, other]} instances={graph.items}>{panel}</TaskScopeProvider> : panel}</aside></>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
