import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { TerminalWorkspace } from '../src/features/terminals/TerminalWorkspace';
import '../src/styles.css';

const cwd = '/workspace/demo';
const pane = (id: string) => ({ id, name: '我的自定义标题', name_is_custom: true, cwd, shell: '/bin/sh', kind: 'terminal', status: 'running', cols: 80, rows: 24 });
const tab = { id: 'tab', name: 'Terminal', workspace_root: cwd,
  layout: { type: 'split', axis: 'row', ratio: .5, first: { type: 'pane', pane_id: 'one' }, second: { type: 'pane', pane_id: 'two' } }, panes: [pane('one'), pane('two')] };

function Preview() {
  const [state, setState] = useState({ tab, detectedAgents: { one: 'codex', two: null },
    terminalTitles: { one: '⠋ 修复终端会话 | demo' }, agentProcesses: { one: { pid: 123, start_time: '1000', cwd } } });
  window.terminalSessionPreview = { update: (patch: object) => setState(value => ({ ...value, ...patch })) };
  return <div className="project-aow-terminal-host" style={{ height: '100vh', width: '100vw' }}>
    <TerminalWorkspace {...state} visible loading={false} onTabChange={tab => setState(value => ({ ...value, tab }))} onTabClosed={() => {}} onPaneStatus={() => {}} onReload={() => {}} />
  </div>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
