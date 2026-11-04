import { t } from '../../lib/i18n';
import { useEffect, useId, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import './settings-action-dialog.css';

export function SettingsActionDialog({ title, busy, onClose, children }: {
  title: string; busy: boolean; onClose: () => void; children: ReactNode;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return createPortal(<dialog ref={dialog} className="project-aow-modal project-aow-dialog settings-action-dialog" aria-labelledby={titleId}
    onCancel={event => { event.preventDefault(); if (!busy) onClose(); }}
    onKeyDown={event => event.stopPropagation()} onPointerDown={event => event.stopPropagation()} onClick={event => event.stopPropagation()}>
    <header><strong id={titleId}>{title}</strong><button type="button" aria-label={t("components.close_dialog")} disabled={busy} onClick={onClose}><X /></button></header>
    {children}
  </dialog>, document.body);
}
