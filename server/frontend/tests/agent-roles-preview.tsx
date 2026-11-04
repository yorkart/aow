import { createRoot } from 'react-dom/client';
import { useRef, useState } from 'react';
import { SettingsDialog } from '../src/features/settings/SettingsDialog';
import { WorkspaceNewMenu } from '../src/aow/WorkspaceNewMenu';
import { TaskDialog } from '../src/features/tasks/TaskDialog';
import { rolesApi } from '../src/features/agent-roles/api';
import type { AowProject } from '../src/aow/types';
import '../src/styles.css';

const project = { id: 'p', name: 'Project', registered_path: '/repo', worktrees: [{ id: 'w', project_id: 'p', path: '/repo', branch: 'main', is_main: true }] } as AowProject;
function Preview() {
  const anchor = useRef<HTMLButtonElement>(null);
  const [view, setView] = useState('settings');
  const [result, setResult] = useState('');
  return <>
    <button onClick={() => setView('settings')}>Settings</button><button ref={anchor} onClick={() => setView('menu')}>Workspace creation</button><button onClick={() => setView('task')}>Create task</button>
    {view === 'settings' && <SettingsDialog agents={[]} embedded onClose={() => setView('')} onReload={async () => {}} onNodesChange={() => {}} />}
    {view === 'menu' && <WorkspaceNewMenu anchor={anchor} agents={[]} onCreateNote={() => {}} onCreateTerminal={() => {}} onCreateAcp={id => setResult(id)} onCreateRole={role => { void rolesApi.launch(role.id, 'p', '/repo').then(value => setResult(JSON.stringify(value))); }} />}
    {view === 'task' && <TaskDialog project={project} onClose={() => setView('')} onSaved={task => { setResult(JSON.stringify(task)); setView(''); }} />}
    <output>{result}</output>
  </>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
