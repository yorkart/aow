import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { InboxPanel } from '../src/features/tasks/InboxPanel';
import { TaskBoard } from '../src/features/tasks/TaskBoard';
import { ConvertDialog } from '../src/features/tasks/ConvertDialog';
import { useTaskBoard } from '../src/features/tasks/api';
import type { InboxItem } from '../src/features/tasks/types';
import type { AowProject } from '../src/aow/types';
import type { AowAgent } from '../src/features/agents/types';
import '../src/styles.css';

const project = { id: 'project', name: 'AoW', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes',
  worktrees: [{ id: 'main', project_id: 'project', path: '/repo', branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' }],
} satisfies AowProject;
const agents = [{ id: 'custom-codex', agent_type: 'codex', display_name: 'Codex', source: 'configured', available: true,
  command: 'codex', executable: '/bin/codex', args: [], env: {} }] satisfies AowAgent[];
const otherProject = { ...project, id: 'other', name: 'Other', registered_path: '/other',
  worktrees: [{ ...project.worktrees[0], id: 'other-main', project_id: 'other', path: '/other' }],
} satisfies AowProject;
function ProjectPreview({ project }: { project: AowProject }) {
  const state = useTaskBoard(project.id, true);
  const [item, setItem] = useState<InboxItem>();
  const [selected, setSelected] = useState<string>();
  const [opened, setOpened] = useState<string>();
  return <div className="project-aow" style={{ display: 'flex', height: '100dvh' }}>
    <main style={{ flex: 1, minWidth: 0 }}><TaskBoard state={state} selectedId={selected} onOpen={task => setOpened(task.tab_id ?? undefined)} />{opened && <output>Opened terminal {opened}</output>}</main>
    <aside style={{ width: 300, flexShrink: 0 }}><InboxPanel state={state} onConvert={setItem} onTask={task => setSelected(task.id)} /></aside>
    {item && state.board && <ConvertDialog item={item} agents={agents} statuses={state.board.statuses} project={project} worktree={project.worktrees[0]} onClose={() => setItem(undefined)} onCreated={task => setSelected(task.id)} />}
  </div>;
}
function Preview() {
  const [current, setCurrent] = useState(project);
  return <><nav aria-label="测试项目"><button onClick={() => setCurrent(project)}>AoW 项目</button><button onClick={() => setCurrent(otherProject)}>Other 项目</button></nav><ProjectPreview key={current.id} project={current} /></>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
