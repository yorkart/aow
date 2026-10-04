import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AowTabEntry } from '../src/aow/AowTabEntry';
import { TerminalWorkspace } from '../src/features/terminals/TerminalWorkspace';
import { MobileTerminals } from '../src/features/terminals/MobileTerminals';
import type { AowProject, AowWorktree } from '../src/aow/types';
import type { TerminalHosting, TerminalTab } from '../src/features/terminals/types';
import '../src/styles.css';
import '../src/mobile/mobile.css';

function Preview() {
  const [tab, setTab] = useState<TerminalTab>();
  const [header, setHeader] = useState<HTMLDivElement | null>(null);
  const [mobile] = useState(() => new URLSearchParams(location.search).has('mobile'));
  useEffect(() => {
    if (!mobile) void fetch('/api/terminals').then(response => response.json()).then(({ tabs }) => setTab(tabs[0]));
  }, [mobile]);
  const worktree = { id: 'worktree', project_id: 'project', path: '/repo', is_main: true, branch: 'main' } as AowWorktree;
  const project = { id: 'project', name: 'Project', worktrees: [worktree] } as AowProject;
  window.hostingPreview = {
    update: (hosting?: TerminalHosting) => setTab(tab => tab && ({ ...tab, panes: [{ ...tab.panes[0], hosting }] })),
  };
  return <AowTabEntry>{entry => <>
    {entry?.target.type === 'automation' && <div data-testid="opened-run">{entry.target.runId}</div>}
    {mobile ? <div className="mobile-app" style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}><div ref={setHeader} className="mobile-header-actions" />
      <MobileTerminals project={project} worktree={worktree} visible headerActions={header} />
    </div> : <div className="project-aow-terminal-host" style={{ height: '100vh', position: 'relative' }}>
      {tab && <TerminalWorkspace visible tab={tab} loading={false} detectedAgents={{ pane: 'codex' }} terminalTitles={{ pane: '实现登录功能' }}
        onTabChange={setTab} onTabClosed={() => {}} onPaneStatus={() => {}} onReload={() => {}} />}
    </div>}
  </>}</AowTabEntry>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
