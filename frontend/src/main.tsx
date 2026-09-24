import { appPath } from './lib/basePath';
import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';
import { LoginGate } from './LoginGate';
import { AgentTaskNotifications } from './features/notifications/AgentTaskNotifications';
import { AowTabEntry } from './aow/AowTabEntry';
import './styles.css';

const mode = new URLSearchParams(window.location.search).get('ui');
const mobile = mode !== 'desktop' && (mode === 'mobile' || /^\/m(?:\/|$)/.test(appPath(window.location.pathname) ?? '')
  || window.matchMedia('(max-width: 820px)').matches);
// Choose once so keyboard and orientation changes never replace a running terminal.
const Aow = mobile
  ? lazy(() => import('./mobile/MobileAow').then((module) => ({ default: module.MobileAow })))
  : lazy(() => import('./aow/desktop'));
const SharedSessionPage = lazy(() => import('./features/sessions/SharedSessionPage').then(module => ({ default: module.SharedSessionPage })));
const shareRoute = /^\/share\/([^/]+)\/?$/.exec(appPath(window.location.pathname) ?? '');
const fallback = <div role="status" style={{ padding: 24, color: '#d7d9de', background: '#181a1f', minHeight: '100dvh', fontFamily: 'system-ui' }}>正在打开 AOW…</div>;

const root = createRoot(document.getElementById('root')!);
root.render(<StrictMode>{shareRoute
  ? <Suspense fallback={fallback}><SharedSessionPage key={shareRoute[1]} token={shareRoute[1]} /></Suspense>
  : <LoginGate><AowTabEntry>{tab => <><Suspense fallback={fallback}><Aow initialEntry={tab} /></Suspense><AgentTaskNotifications /></>}</AowTabEntry></LoginGate>
}</StrictMode>);
