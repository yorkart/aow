import { StrictMode, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { SessionSnapshotView } from '../src/features/sessions/SessionSnapshotView';
import { SessionShareButton } from '../src/features/sessions/SessionShareButton';
import { MobileSessionReader } from '../src/features/sessions/MobileSessions';
import { session, snapshot } from './fixtures/session-snapshot.mjs';
import '../src/styles.css';

const mobile = new URLSearchParams(location.search).has('mobile');
if (mobile) await import('../src/mobile/mobile.css');

function Preview() {
  const [state, setState] = useState({ session, snapshot, loading: false, error: undefined });
  window.sessionPreview = { update: (patch) => setState((current) => ({ ...current, ...patch })) };
  const refresh = () => setState((current) => ({ ...current, loading: false, error: undefined,
    snapshot: { ...current.snapshot, captured_at: new Date().toISOString() } }));
  return mobile ? <div className="mobile-app"><MobileSessionReader session={state.session} workspace={session.cwd} back={() => {}} /></div>
    : <SessionSnapshotView {...state} onRefresh={refresh} actions={<SessionShareButton session={state.session} workspacePath={session.cwd} />} />;
}

createRoot(document.getElementById('root')!).render(<StrictMode><Preview /></StrictMode>);
