import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { TerminalPanel } from '../src/features/terminals/TerminalPanel';
import { terminalApi } from '../src/features/terminals/terminalApi';
import type { TerminalTab } from '../src/features/terminals/types';
import type { AowWorktree } from '../src/aow/types';
import { TabNavigationContext } from '../src/aow/tabNavigation';
import '../src/styles.css';

function Preview() {
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [currentOnly, setCurrentOnly] = useState(false);
  const [opened, setOpened] = useState('');
  const reload = () => { void terminalApi.list().then(setTabs); };
  useEffect(reload, []);
  const worktrees = ['/repo', '/auth', '/tests'].map((path, index) => ({
    id: path, project_id: 'project', path, branch: ['main', 'feat/auth', 'feat/tests'][index], is_main: index === 0,
  })) as AowWorktree[];
  return <div className="project-aow" style={{ display: 'block', height: '100vh' }}>
    <button onClick={() => setCurrentOnly(value => !value)}>当前 worktree</button>
    <output aria-label="Opened pane">{opened}</output>
    <div style={{ width: 'min(380px, 100vw)', height: 'calc(100vh - 30px)' }}>
      <TabNavigationContext.Provider value={async target => setOpened(target.type === 'acp' ? `acp:${target.thread_id}` : target.type === 'terminal' ? `terminal:${target.tabId}` : '')}>
      <TerminalPanel tabs={currentOnly ? tabs.filter(tab => tab.workspace_root === '/repo') : tabs} descendantTabs={tabs}
        worktrees={worktrees} activeWorktreePath="/repo"
        detectedAgents={{}} titles={{}} openedIds={new Set()} onReload={reload}
        onOpen={tab => setOpened(tab.id)} onOpenPane={(tab, paneId) => setOpened(`${tab.id}:${paneId}`)}
        onTerminate={async () => {}} onRebuild={async () => {}} />
      </TabNavigationContext.Provider>
    </div>
  </div>;
}

createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
