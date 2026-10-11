import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AgentTaskNotifications } from '../src/features/notifications/AgentTaskNotifications';
import { NotificationSettingsPanel } from '../src/features/notifications/NotificationSettingsPanel';
import { AowTabEntry, useAowTabLocation } from '../src/aow/AowTabEntry';
import type { ResolvedTab } from '../src/aow/tabRoutes';
import { AcpPanel } from '../src/features/acp';
import { useUnreadTerminalIds, useWorktreeUnreadCounts } from '../src/features/notifications/taskNotifications';
import '../src/styles.css';

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
    <AcpPanel workspace="/workspace/demo" visible onOpen={session => syncTab({ type: 'acp', thread_id: session.thread_id })} onDeleted={() => {}} />
    <button disabled={busy} onClick={() => setSection('im')}>IM 设置</button>
    <button disabled={busy} onClick={() => setSection('notifications')}>通知设置</button>
    {section && <section className="project-aow-modal project-aow-dialog"><NotificationSettingsPanel section={section} onBusyChange={setBusy} /></section>}
  </>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><AowTabEntry>{entry => <Preview entry={entry} />}</AowTabEntry></StrictMode>);
