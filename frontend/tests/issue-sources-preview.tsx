import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { InboxPanel } from '../src/features/tasks/InboxPanel';
import { useTaskBoard } from '../src/features/tasks/api';
import '../src/styles.css';

function Preview() {
  const [project, setProject] = useState('project');
  const state = useTaskBoard(project, true);
  return <div className="project-aow" style={{ height: '100dvh', maxWidth: 360, display: 'flex', flexDirection: 'column' }}>
    <nav><button onClick={() => setProject('project')}>AoW 项目</button><button onClick={() => setProject('other')}>Other 项目</button></nav>
    <div style={{ flex: 1, minHeight: 0 }}><InboxPanel key={project} state={state} onConvert={() => {}} onTask={() => {}} /></div>
  </div>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
