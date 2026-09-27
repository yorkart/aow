import { StrictMode, useCallback, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AutomationPanel } from '../src/features/automations/AutomationPanel';
import { AutomationDetail } from '../src/features/automations/AutomationDetail';
import type { AutomationTask } from '../src/features/automations/types';
import type { AowProject } from '../src/aow/types';
import type { AowAgent } from '../src/features/agents/types';
import '../src/styles.css';

const project = { id: 'project', name: 'Project', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes',
  worktrees: [{ id: 'main', project_id: 'project', path: '/repo', branch: 'main', head: 'abc', is_main: true, detached: false, locked: false, prunable: false, color: 'default' }],
} satisfies AowProject;
const agents = [{ id: 'codex', agent_type: 'codex', display_name: 'Codex', source: 'configured', available: true,
  command: 'codex', executable: '/bin/codex', args: [], env: {} }] satisfies AowAgent[];
function Preview() {
  const [selected, setSelected] = useState<AutomationTask>();
  const [refresh, setRefresh] = useState(0);
  const changed = useCallback((_task?: AutomationTask, mutated?: boolean) => { if (mutated) setRefresh(value => value + 1); }, []);
  const deleted = useCallback(() => setSelected(undefined), []);
  return <div className="project-aow" style={{ display: 'flex', height: '100vh' }}>
    <main style={{ flex: 1, minWidth: 0 }}>{selected ? <AutomationDetail taskId={selected.id} visible project={project} agents={agents} refreshKey={refresh}
      onTaskChanged={changed} onTaskDeleted={deleted} onOpenSession={() => {}} /> : null}</main>
    <aside style={{ width: 310 }}><AutomationPanel project={project} agents={agents} activeTaskId={selected?.id} refreshKey={refresh}
      onOpenTask={setSelected} onTaskChanged={changed} onTaskDeleted={deleted} /></aside>
  </div>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
