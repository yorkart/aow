import { t } from '../../lib/i18n';
import { useEffect, useId, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, Copy, LoaderCircle, Share2, X } from 'lucide-react';
import { sessionShareApi, type SessionShare } from './sessionShareApi';
import { ConversationAction } from '../../components/conversation/ConversationAction';
import type { AowAgentSession } from './types';
import './session-share.css';

interface Props { session: AowAgentSession; workspacePath: string }

export function SessionShareButton(props: Props) {
  const [open, setOpen] = useState(false);
  return <>
    <ConversationAction className="session-share-button" label={t("sessions.share")} onClick={() => setOpen(true)}><Share2 size={16} /></ConversationAction>
    {open && <SessionShareDialog key={`${props.session.id}:${props.workspacePath}`} {...props} close={() => setOpen(false)} />}
  </>;
}

export function SessionShareDialog({ session, workspacePath, close }: Props & { close: () => void }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const linkRef = useRef<HTMLInputElement>(null);
  const busyRef = useRef(false);
  const [share, setShare] = useState<SessionShare | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const id = useId();
  const url = share ? new URL(share.path, window.location.origin).href : '';

  useEffect(() => {
    const dialog = dialogRef.current!;
    dialog.showModal();
    const controller = new AbortController();
    void sessionShareApi.info(session, controller.signal).then(setShare).catch(reason => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : t("sessions.unable_to_read_sharing_settings"));
    }).finally(() => { if (!controller.signal.aborted) setLoading(false); });
    return () => { controller.abort(); dialog.close(); };
  }, [session.agent, session.session_id]);

  const copy = async (value: string) => {
    try {
      if (!navigator.clipboard?.writeText) throw new Error(t("common.clipboard_unavailable_2"));
      await navigator.clipboard.writeText(value);
      setNotice(t("sessions.link_copied"));
    } catch {
      // HTTP deployments may not expose Clipboard API. Keep the link selectable.
      const input = linkRef.current;
      if (input) { input.value = value; input.focus(); input.select(); }
      try {
        if (!input || !document.execCommand('copy')) throw new Error(t("common.copy_failed"));
        setNotice(t("sessions.link_copied"));
      } catch { setNotice(t("sessions.link_generated_select_it_and_copy_manually")); }
    }
  };

  const mutate = async (action: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true); setError(''); setNotice('');
    try { await action(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : t("sessions.action_failed_try_again")); }
    finally { busyRef.current = false; setBusy(false); }
  };

  const createAndCopy = () => mutate(async () => {
    const result = share ?? await sessionShareApi.create(session, workspacePath);
    setShare(result);
    await copy(new URL(result.path, window.location.origin).href);
  });
  const revoke = () => mutate(async () => {
    if (!share) return;
    await sessionShareApi.revoke(share.id);
    setShare(null);
    setNotice(t("sessions.sharing_canceled_the_old_link_is_no_longer_valid"));
  });

  return createPortal(<dialog ref={dialogRef} className="session-share-dialog" aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`}
    onCancel={event => { event.preventDefault(); if (!busyRef.current) close(); }} onKeyDown={event => event.stopPropagation()}>
    <header><h2 id={`${id}-title`}>{t("sessions.share_session")}</h2><button type="button" aria-label={t("sessions.close_sharing_window")} disabled={busy} onClick={close}><X size={18} /></button></header>
    <p className="session-share-title">{session.title}</p>
    <p id={`${id}-description`}>{t("sessions.anyone_with_the_link_can_view_this_session_and")}</p>
    <label htmlFor={`${id}-link`}>{t("sessions.share_link")}</label>
    <input ref={linkRef} id={`${id}-link`} value={url} readOnly placeholder={loading ? t("sessions.reading_sharing_settings") : t("sessions.the_share_link_appears_after_creation")} onFocus={event => event.currentTarget.select()} />
    {error && <p className="session-share-error" role="alert">{error}</p>}
    {notice && <p className="session-share-notice" role="status">{notice === t("sessions.link_copied") && <Check size={14} />}{notice}</p>}
    <footer>{share && <button type="button" className="danger" disabled={busy || loading} onClick={() => void revoke()}>{t("sessions.stop_sharing")}</button>}
      <button type="button" className="primary" disabled={busy || loading} onClick={() => void createAndCopy()}>
        {busy || loading ? <LoaderCircle size={15} className="spinning" /> : <Copy size={15} />}{share ? t("sessions.copy_link") : t("sessions.create_and_copy_link")}
      </button></footer>
  </dialog>, document.body);
}
