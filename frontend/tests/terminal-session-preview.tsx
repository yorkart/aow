import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { TerminalWorkspace } from '../src/features/terminals/TerminalWorkspace';
import { Explorer } from '../src/features/files/Explorer';
import '../src/styles.css';

const cwd = '/workspace/demo';
const pane = (id: string) => ({ id, name: '我的自定义标题', name_is_custom: true, cwd, shell: '/bin/sh', kind: 'terminal', status: 'running', cols: 80, rows: 24 });
const tab = { id: 'tab', name: 'Terminal', workspace_root: cwd,
  layout: { type: 'split', axis: 'row', ratio: .5, first: { type: 'pane', pane_id: 'one' }, second: { type: 'pane', pane_id: 'two' } }, panes: [pane('one'), pane('two')] };

function Preview() {
  const explorer = new URLSearchParams(location.search).has('explorer');
  const [state, setState] = useState({ tab, detectedAgents: { one: 'codex', two: null },
    terminalTitles: { one: '⠋ 修复终端会话 | demo' }, agentProcesses: { one: { pid: 123, start_time: '1000', cwd } } });
  window.terminalSessionPreview = { update: (patch: object) => setState(value => ({ ...value, ...patch })) };
  return <div style={{ display: 'flex', height: '100vh', width: '100vw' }}>
    {explorer ? <div style={{ display: 'flex', width: 260, flexShrink: 0 }}>
      <Explorer root={cwd} lockedRoot onRootChange={() => {}} onOpenFile={() => {}}
        refreshRequest={{ generation: 0, directory: cwd }} />
    </div> : null}
    <div className="project-aow-terminal-host" style={{ position: 'relative', flex: 1, minWidth: 0 }}>
      <TerminalWorkspace {...state} visible loading={false} onTabChange={tab => setState(value => ({ ...value, tab }))} onTabClosed={() => {}} onPaneStatus={() => {}} onReload={() => {}} />
    </div>
  </div>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
