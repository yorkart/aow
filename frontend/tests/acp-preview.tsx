import { useState } from 'react';
import { WorkspaceMarkdownProvider } from '../src/features/files/WorkspaceMarkdown';
import { createRoot } from 'react-dom/client';
import { createPortal } from 'react-dom';
import { ProjectAcpPanelProvider, AcpPanel, AcpSessionTab, AcpSettings, useAcpTabs } from '../src/features/acp';
import { WorkspaceTabs, type WorkspaceTab } from '../src/aow/WorkspaceTabs';
import '../src/styles.css';
const project = { id: 'acp-preview', name: 'ACP', registered_path: '/repo', common_git_dir: '/repo/.git', notes_path: '/notes',
  worktrees: ['/repo', '/repo-feature'].map((path, index) => ({ id: path, project_id: 'acp-preview', path, branch: index ? 'feature' : 'main', head: 'abc', is_main: !index, detached: false, locked: false, prunable: false, color: 'default' as const })) };
function Preview() {
  const [opened, setOpened] = useState('');
  const [dirty, setDirty] = useState(false);
  const [activeId, setActiveId] = useState('');
  const [moved, setMoved] = useState(false);
  const [portal, setPortal] = useState<HTMLDivElement | null>(null);
  const acp = useAcpTabs('/repo');
  const tabs: WorkspaceTab[] = acp.tabs.map(tab => ({ id: tab.id, kind: 'acp', label: tab.title, targetId: tab.id, acpAgentId: tab.agentId, acpWorking: tab.working }));
  const settings = new URLSearchParams(location.search).has('settings');
  const close = (ids: string[]) => { acp.close(ids); if (ids.includes(activeId)) setActiveId(acp.tabs.find(tab => !ids.includes(tab.id))?.id || ''); };
  return <div style={{ height: '100vh', width: settings ? 650 : '100%', background: '#202124', color: '#ddd', '--aow-text': '#ddd', '--aow-line': '#444', '--aow-panel': '#252629', '--aow-muted': '#aaa', '--aow-accent': '#82baff' } as React.CSSProperties}>
    {settings ? <AcpSettings active onBusyChange={() => undefined} onDirtyChange={setDirty} /> : <div style={{ display: 'flex', height: '100%' }}>
      <main style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1 }}>
        <WorkspaceTabs workspaceKey="acp-preview" tabs={tabs} activeId={activeId} visible agents={[]} onActivate={tab => setActiveId(tab.id)} onCloseTab={tab => close([tab.id])} onCloseTabs={items => close(items.map(item => item.id))} onCanRename={() => false} onRename={() => undefined} onError={() => undefined} destination={() => undefined} onOpenElsewhere={() => undefined} onCreateNote={() => undefined} onCreateTerminal={() => undefined} onCreateAcp={agent => setActiveId(acp.create(agent))} />
        <button type="button" onClick={() => setMoved(value => !value)}>移动会话视图</button>
        <div style={{ flex: 1, position: 'relative' }} hidden={moved}>{acp.tabs.map(tab => {
          const content = <div key={tab.id} className="acp-tab-host" hidden={activeId !== tab.id}>
            <AcpSessionTab tab={tab} workspace="/repo" visible={activeId === tab.id} onSessionChange={acp.changed} onOpenSession={session => setActiveId(acp.open(session))} onOpenFile={(path, line) => setOpened(`${path}:${line}`)} />
          </div>;
          return moved && portal ? createPortal(content, portal, tab.id) : content;
        })}</div>
        <div ref={setPortal} style={{ flex: 1, position: 'relative' }} hidden={!moved} />
      </main>
      <aside style={{ width: 300 }}><ProjectAcpPanelProvider project={project}><AcpPanel workspace="/repo" visible activeThreadId={acp.tabs.find(tab => tab.id === activeId)?.thread_id} onOpen={session => setActiveId(acp.open(session))} /></ProjectAcpPanelProvider></aside>
    </div>}
    <output hidden aria-label="Opened file">{opened}</output><output hidden aria-label="Unsaved settings">{String(dirty)}</output>
  </div>;
}
createRoot(document.getElementById('root')!).render(<WorkspaceMarkdownProvider workspace="fixture-workspace" directory="/repo"><Preview /></WorkspaceMarkdownProvider>);
