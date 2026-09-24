import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { AgentTaskNotifications } from '../src/features/notifications/AgentTaskNotifications';
import { NotificationSettingsPanel } from '../src/features/notifications/NotificationSettingsPanel';
import { AowTabEntry } from '../src/aow/AowTabEntry';
import '../src/styles.css';

function Preview() {
  const [mounted, setMounted] = useState(true);
  const [section, setSection] = useState<'im' | 'notifications'>();
  const [busy, setBusy] = useState(false);
  return <><button onClick={() => setMounted(value => !value)}>切换订阅</button>
    <input aria-label="继续工作" />{mounted && <AgentTaskNotifications />}
    <button disabled={busy} onClick={() => setSection('im')}>IM 设置</button>
    <button disabled={busy} onClick={() => setSection('notifications')}>通知设置</button>
    {section && <section className="project-aow-modal project-aow-dialog"><NotificationSettingsPanel section={section} onBusyChange={setBusy} /></section>}
  </>;
}
createRoot(document.getElementById('root')!).render(<StrictMode><AowTabEntry>{() => <Preview />}</AowTabEntry></StrictMode>);
