import { t, getLanguage } from '../../lib/i18n';
import { LiveEvents } from '../../lib/liveEvents';
import { TokenUsageBadge } from '../../components/TokenUsageBadge';
import { useEffect, useId, useRef, useState } from 'react';
import { ChevronDown, ChevronUp, Layers, X } from 'lucide-react';
import './agent-task-notifications.css';
import { useAowTaskTabActivity } from '../../aow/AowTabEntry';
import { noticeMatchesTarget, parseTaskStop, taskNotifications, useTaskNotifications, type TaskNotice } from './taskNotifications';
import { useTaskNotice } from './useTaskNotice';
import { AgentIcon } from '../agents/AgentIcon';

const agentNames: Record<string, string> = { codex: 'Codex', traecli: 'TraeCode CLI', claude: 'Claude Code', hermes: 'Hermes', pi: 'Pi' };
const visibleNoticeCount = 5;

function TaskNoticeCard({ notice }: { notice: TaskNotice }) {
  const { projectName, tabName, failure, opening, canOpen, open, dismiss } = useTaskNotice(notice);
  return <article className="agent-task-notice" role="status">
    <button type="button" className="agent-task-notice-open" aria-label={t("notifications.open_notification", { value0: projectName, value1: tabName })}
      disabled={opening || !canOpen} aria-busy={opening} onClick={() => void open()}>
      <span className="agent-task-notice-agent" title={agentNames[notice.agent] ?? notice.agent}><AgentIcon agentId={notice.agent.replace(/-acp$/, '').replace(/^claude-agent$/, 'claude')} /></span>
      <span className="agent-task-notice-content">
        <span className="agent-task-notice-heading"><strong title={projectName}>{projectName}</strong>
          <time dateTime={new Date(notice.receivedAt).toISOString()} title={new Date(notice.receivedAt).toLocaleString(getLanguage())}>
            {new Date(notice.receivedAt).toLocaleTimeString(getLanguage(), { hour: '2-digit', minute: '2-digit' })}
          </time>
        </span>
        <span className="agent-task-notice-tab" title={tabName}>{tabName}</span>
        <TokenUsageBadge usage={notice.usage} />
      </span>
    </button>
    <button type="button" className="agent-task-notice-close" aria-label={t("notifications.dismiss_notification", { value0: projectName, value1: tabName })}
      title={t("notifications.close_popup_and_keep_unread_reminder")} disabled={opening}
      onClick={() => taskNotifications.closePopups(item => item.key === notice.key)}><X size={14} aria-hidden="true" /></button>
    {failure && <div className="agent-task-notice-failure">
      <span className="agent-task-notice-error" role="alert">{failure}</span>
      <button type="button" className="agent-task-notice-remove" onClick={dismiss}>{t("notifications.remove_notification")}</button>
    </div>}
  </article>;
}

export function AgentTaskNotifications() {
  const { notices: unreadNotices, storageError } = useTaskNotifications();
  const notices = unreadNotices.filter(notice => !notice.popupClosed);
  const [expanded, setExpanded] = useState(false);
  const tabActivity = useAowTaskTabActivity();
  const listId = useId();
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => { if (!notices.length) setExpanded(false); }, [notices.length]);

  useEffect(() => {
    const dismissActive = () => {
      const target = tabActivity.getCurrent();
      if (target) taskNotifications.dismiss(notice => !!noticeMatchesTarget(notice, target));
    };
    dismissActive();
    return tabActivity.subscribe(dismissActive);
  }, [tabActivity]);

  useEffect(() => {
    const source = new LiveEvents('task-stopped');
    const stopped = (event: Event) => {
      let notice;
      try { notice = parseTaskStop(JSON.parse((event as MessageEvent<string>).data)); } catch { return; }
      if (!notice) return;
      if (noticeMatchesTarget(notice, tabActivity.getCurrent())) return;
      // This is a live callback, not session-level deduplication: every newly
      // consumed stop record gets its own notice, including subsequent turns.
      taskNotifications.add(notice);
    };
    source.addEventListener('task-stopped', stopped);
    return () => { source.removeEventListener('task-stopped', stopped); source.close(); };
  }, [tabActivity]);

  if (!notices.length) return null;
  const visibleNotices = expanded ? notices : notices.slice(0, visibleNoticeCount);
  const foldedCount = notices.length - visibleNoticeCount;
  const collapse = () => { setExpanded(false); list.current?.scrollTo({ top: 0 }); };
  return <section className={`agent-task-notifications${expanded ? ' is-expanded' : ''}`} aria-label={t("notifications.task_completion_reminder")}
    onKeyDown={event => { if (expanded && event.key === 'Escape') { event.stopPropagation(); collapse(); } }}>
    {(notices.length > 1 || expanded) && <div className="agent-task-notifications-toolbar">
      <span>{notices.length} {t("notifications.pending")}</span>
      {expanded && <button onClick={collapse} aria-expanded="true" aria-controls={listId}><ChevronUp size={14} />{t("aow.collapse")}</button>}
      {notices.length > 1 && <button className="agent-task-notifications-clear" title={t("notifications.close_all_popups_and_keep_unread_reminders")} onClick={() => taskNotifications.closePopups(() => true)}>{t("notifications.close_all")}</button>}
    </div>}
    {storageError && <div className="agent-task-notifications-storage-error" role="alert">{storageError}</div>}
    <div className="agent-task-notifications-list" id={listId} ref={list} tabIndex={expanded ? 0 : undefined}
      aria-label={t("notifications.pending_notifications")} aria-live="polite" aria-relevant="additions">
      {visibleNotices.map(notice => <TaskNoticeCard key={notice.key} notice={notice} />)}
    </div>
    {!expanded && foldedCount > 0 && <button className="agent-task-notifications-stack" aria-expanded="false" aria-controls={listId}
      onClick={() => { setExpanded(true); list.current?.scrollTo({ top: 0 }); }}>
      <Layers size={18} aria-hidden="true" /><span><strong>{t("notifications.more")} {foldedCount} {t("notifications.notifications")}</strong><small>{t("notifications.expand_to_view_all")}</small></span><ChevronDown size={16} aria-hidden="true" />
    </button>}
  </section>;
}
