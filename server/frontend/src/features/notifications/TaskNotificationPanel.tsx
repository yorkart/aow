import { t, getLanguage } from '../../lib/i18n';
import { useEffect, useRef, useState } from 'react';
import { Bell, ExternalLink, Trash2, X } from 'lucide-react';
import { AgentIcon } from '../agents/AgentIcon';
import { TokenUsageBadge } from '../../components/TokenUsageBadge';
import { taskNotifications, useTaskNotifications, type TaskNotice } from './taskNotifications';
import { useTaskNotice } from './useTaskNotice';
import './task-notification-panel.css';

function NotificationRow({ notice }: { notice: TaskNotice }) {
  const { projectName, tabName, failure, opening, canOpen, open, dismiss } = useTaskNotice(notice);
  const workspaces = [...new Set(notice.sources.map(source => source.workspace_root).filter(Boolean))].join('、') || notice.cwd;
  return <article className="task-notification-row">
    <span className="task-notification-agent" title={notice.agent}><AgentIcon agentId={notice.agent.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /></span>
    <div className="task-notification-content">
      <div className="task-notification-heading"><strong>{notice.title || tabName}</strong>
        <time dateTime={new Date(notice.receivedAt).toISOString()}>{new Date(notice.receivedAt).toLocaleString(getLanguage())}</time>
      </div>
      <div>{projectName} · {tabName}</div>
      <small className="task-notification-workspace">{workspaces}</small>
      <TokenUsageBadge usage={notice.usage} />
      {failure && <div className="task-notification-error" role="alert">{failure}</div>}
    </div>
    <div className="task-notification-actions">
      <button type="button" aria-label={t("notifications.open_reminder", { value0: projectName, value1: tabName })} title={t("notifications.open_source_tab")} disabled={opening || !canOpen} aria-busy={opening}
        onClick={() => void open()}><ExternalLink size={14} />{t("notifications.view")}</button>
      <button type="button" aria-label={t("notifications.remove_reminder", { value0: projectName, value1: tabName })} title={t("notifications.force_clear_this_reminder")} disabled={opening} onClick={dismiss}><Trash2 size={14} />{t("notifications.remove")}</button>
    </div>
  </article>;
}

export function TaskNotificationPanel({ onClose }: { onClose: () => void }) {
  const { notices, storageError } = useTaskNotifications();
  const [height, setHeight] = useState(320);
  const resize = useRef<{ y: number; height: number } | undefined>(undefined);
  const panel = useRef<HTMLElement>(null);
  useEffect(() => { panel.current?.focus({ preventScroll: true }); }, []);
  const resizeTo = (value: number) => setHeight(Math.max(180, Math.min(window.innerHeight * 0.75, value)));
  const clear = () => {
    // Clear the displayed records; a later incoming event must remain unread.
    const keys = new Set(notices.map(notice => notice.key));
    taskNotifications.dismiss(notice => keys.has(notice.key));
  };
  return <section id="task-notification-panel" ref={panel} tabIndex={-1} className="task-notification-panel" aria-label={t("aow.reminder")} style={{ height }} onKeyDown={event => {
    if (event.key === 'Escape') { event.stopPropagation(); onClose(); }
  }}>
    <div className="task-notification-resize" role="separator" aria-label={t("notifications.resize_reminders_panel_height")} aria-orientation="horizontal" tabIndex={0}
      onPointerDown={event => { resize.current = { y: event.clientY, height }; event.currentTarget.setPointerCapture(event.pointerId); }}
      onPointerMove={event => { if (resize.current) resizeTo(resize.current.height + resize.current.y - event.clientY); }}
      onPointerUp={() => { resize.current = undefined; }} onPointerCancel={() => { resize.current = undefined; }}
      onKeyDown={event => { if (event.key === 'ArrowUp' || event.key === 'ArrowDown') { event.preventDefault(); resizeTo(height + (event.key === 'ArrowUp' ? 20 : -20)); } }} />
    <header><div><Bell size={15} /><strong>{t("aow.reminder")}</strong></div>
      <div><button type="button" title={t("notifications.force_clear_all_pending_reminders")} disabled={!notices.length} onClick={clear}><Trash2 size={14} />{t("notifications.clear_all")}</button>
        <button type="button" aria-label={t("notifications.close_reminders_panel")} title={t("aow.close")} onClick={onClose}><X size={15} /></button></div>
    </header>
    {storageError && <div className="task-notification-error" role="alert">{storageError}</div>}
    <div className="task-notification-list">
      {notices.map(notice => <NotificationRow key={notice.key} notice={notice} />)}
      {!notices.length && <div className="task-notification-empty">{t("notifications.no_pending_reminders")}</div>}
    </div>
    <footer>{notices.length} {t("notifications.pending_reminders_includes_reminders_with_closed_popups")}</footer>
  </section>;
}
