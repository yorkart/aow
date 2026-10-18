import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AgentTaskNotifications } from '../src/features/notifications/AgentTaskNotifications';
import { NotificationSettingsPanel } from '../src/features/notifications/NotificationSettingsPanel';
import { AowTabEntry, useAowTabLocation } from '../src/aow/AowTabEntry';
import type { ResolvedTab } from '../src/aow/tabRoutes';
import { ProjectAcpPanelProvider, AcpPanel } from '../src/features/acp';
import { useUnreadTerminalIds, useWorktreeUnreadCounts } from '../src/features/notifications/taskNotifications';
import '../src/styles.css';

const project = { id: 'acp-preview', name: 'ACP', registered_path: '/workspace/demo', common_git_dir: '/workspace/demo/.git', notes_path: '/notes',
  worktrees: ['/workspace/demo', '/workspace/demo-feature'].map((path, index) => ({ id: path, project_id: 'acp-preview', path, branch: index ? 'feature' : 'main', head: 'abc', is_main: !index, detached: false, locked: false, prunable: false, color: 'default' as const })) };
function Preview({ entry }: { entry?: ResolvedTab }) {
  const syncTab = useAowTabLocation();
  const unreadTerminals = useUnreadTerminalIds();
  const unreadWorktrees = useWorktreeUnreadCounts();
  useEffect(() => { if (entry) syncTab(entry.target); }, [entry, syncTab]);
  const [mounted, setMounted] = useState(true);
  const [section, setSection] = useState<'im' | 'notifications'>();
  const [busy, setBusy] = useState(false);
  return <><button onClick={() => setMounted(value => !value)}>切换订阅</button>
    <input aria-label="继续工作" />{mounted && <AgentTaskNotifications />}
    <button onClick={() => syncTab()}>离开当前会话</button>
    <button onClick={() => syncTab({ type: 'terminal', tabId: 'shared' })}>查看同名 Terminal</button>
    <output aria-label="Terminal 未读数">{unreadTerminals.size}</output>
    <output aria-label="工作区未读数">{unreadWorktrees.get('/workspace/demo') ?? 0}</output>
    <ProjectAcpPanelProvider project={project}><AcpPanel workspace="/workspace/demo" visible onOpen={session => syncTab({ type: 'acp', thread_id: session.thread_id })} /></ProjectAcpPanelProvider>
    <button disabled={busy} onClick={() => setSection('im')}>IM 设置</button>
    <button disabled={busy} onClick={() => setSection('notifications')}>通知设置</button>
    {section && <section className="project-aow-modal project-aow-dialog"><NotificationSettingsPanel section={section} onBusyChange={setBusy} /></section>}
  </>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><AowTabEntry>{entry => <Preview entry={entry} />}</AowTabEntry></StrictMode>);
