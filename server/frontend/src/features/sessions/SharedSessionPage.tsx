import { t } from '../../lib/i18n';
import { useCallback, useEffect, useRef, useState } from 'react';
import { AlertCircle, LoaderCircle, Share2 } from 'lucide-react';
import { SessionSnapshotView } from './SessionSnapshotView';
import { sessionShareApi, SessionShareError } from './sessionShareApi';
import type { AgentSessionSnapshot } from './types';
import './session-share.css';

export function SharedSessionPage({ token }: { token: string }) {
  const [snapshot, setSnapshot] = useState<AgentSessionSnapshot>();
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [unavailable, setUnavailable] = useState(false);
  const reloadRef = useRef<() => void>(() => {});
  const refresh = useCallback(() => reloadRef.current(), []);

  useEffect(() => {
    let active = true;
    let stopped = false;
    let request: AbortController | undefined;
    let timer: number | undefined;
    const update = async () => {
      if (!active || stopped || request || document.visibilityState !== 'visible') return;
      window.clearTimeout(timer);
      const controller = new AbortController();
      request = controller;
      setLoading(true);
      try {
        const next = await sessionShareApi.read(token, controller.signal);
        if (active && !controller.signal.aborted) { setSnapshot(next); setError(''); }
      } catch (reason) {
        if (!active || controller.signal.aborted) return;
        if (reason instanceof SessionShareError && (reason.status === 404 || reason.status === 410)) {
          stopped = true;
          setSnapshot(undefined);
          setUnavailable(true);
        }
        setError(reason instanceof Error ? reason.message : t("sessions.failed_to_read_session_try_again_later"));
      } finally {
        // A hidden tab may abort this request and start another one immediately
        // on becoming visible. Its old completion must not replace the new timer.
        if (active && request === controller) {
          request = undefined;
          setLoading(false);
          if (!stopped && document.visibilityState === 'visible') timer = window.setTimeout(() => void update(), 10_000);
        }
      }
    };
    const visibility = () => {
      if (document.visibilityState === 'visible') void update();
      else {
        window.clearTimeout(timer);
        const pending = request;
        request = undefined;
        pending?.abort();
      }
    };
    reloadRef.current = () => void update();
    document.addEventListener('visibilitychange', visibility);
    void update();
    return () => { active = false; request?.abort(); window.clearTimeout(timer); document.removeEventListener('visibilitychange', visibility); };
  }, [token]);

  return <main className="shared-session-page">
    <div className="shared-session-banner"><Share2 size={15} /><strong>{t("sessions.shared_session_read_only")}</strong><span>{t("sessions.content_updates_automatically_every_10_seconds")}</span></div>
    {snapshot ? <SessionSnapshotView session={{ id: `${snapshot.agent}:${snapshot.session_id}`, agent: snapshot.agent, title: snapshot.title }}
      snapshot={snapshot} loading={loading} error={error} onRefresh={refresh} publicView />
      : <div className="shared-session-state" role={error ? 'alert' : 'status'}>
        {loading ? <LoaderCircle className="spinning" /> : <AlertCircle />}
        <p>{error || t("sessions.reading_shared_session")}</p>
        {!loading && !unavailable && <button type="button" onClick={refresh}>{t("aow.retry")}</button>}
      </div>}
  </main>;
}
