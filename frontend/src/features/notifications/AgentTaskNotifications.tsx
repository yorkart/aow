import { appUrl } from '../../lib/basePath';
import { useEffect, useId, useRef, useState } from 'react';
import { ChevronDown, ChevronUp, Layers, X } from 'lucide-react';
import './agent-task-notifications.css';
import { useAowTerminalTabActivity, useAowTabNavigation } from '../../aow/AowTabEntry';
import { parseTaskStop, taskNotifications, useTaskNotifications, type TaskNotice } from './taskNotifications';
import { AgentIcon } from '../agents/AgentIcon';

const agentNames: Record<string, string> = { codex: 'Codex', traecli: 'TraeCode CLI', claude: 'Claude Code', hermes: 'Hermes' };
const visibleNoticeCount = 5;

function TaskNoticeCard({ notice }: { notice: TaskNotice }) {
  const navigateTab = useAowTabNavigation();
  const [error, setError] = useState('');
  const [opening, setOpening] = useState(false);
  const sources = notice.sources.filter(source => source.tab_id);
  const projectName = [...new Set(sources.map(source => source.project_name || source.workspace_root.split('/').filter(Boolean).pop() || '未命名项目'))].join('、')
    || notice.cwd.split('/').filter(Boolean).pop() || '未命名项目';
  const tabName = [...new Set(sources.map(source => source.tab_name || '未命名 Tab'))].join('、') || 'Tab 不可用';
  const failure = error || (!sources.length ? '无法定位对应 Tab。' : '');
  const dismiss = () => taskNotifications.dismiss(item => item.key === notice.key);
  const open = async () => {
    if (opening) return;
    setOpening(true);
    setError('');
    // A session may have several source tabs. Open the first one still available.
    let message = '';
    for (const source of sources) {
      try {
        await navigateTab({ type: 'terminal', tabId: source.tab_id });
        dismiss();
        setOpening(false);
        return;
      } catch (reason) { message = reason instanceof Error ? reason.message : String(reason); }
    }
    setError(message);
    setOpening(false);
  };
  return <article className="agent-task-notice" role="status">
    <button type="button" className="agent-task-notice-open" aria-label={`打开通知：${projectName} · ${tabName}`}
      disabled={opening || !sources.length} aria-busy={opening} onClick={() => void open()}>
      <span className="agent-task-notice-agent" title={agentNames[notice.agent] ?? notice.agent}><AgentIcon agentId={notice.agent} /></span>
      <span className="agent-task-notice-content">
        <span className="agent-task-notice-heading"><strong title={projectName}>{projectName}</strong>
          <time dateTime={new Date(notice.receivedAt).toISOString()} title={new Date(notice.receivedAt).toLocaleString()}>
            {new Date(notice.receivedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
          </time>
        </span>
        <span className="agent-task-notice-tab" title={tabName}>{tabName}</span>
      </span>
    </button>
    <button type="button" className="agent-task-notice-close" aria-label={`关闭通知：${projectName} · ${tabName}`}
      title="关闭弹框，保留未读提醒" disabled={opening}
      onClick={() => taskNotifications.closePopups(item => item.key === notice.key)}><X size={14} aria-hidden="true" /></button>
    {failure && <div className="agent-task-notice-failure">
      <span className="agent-task-notice-error" role="alert">{failure}</span>
      <button type="button" className="agent-task-notice-remove" onClick={dismiss}>移除通知</button>
    </div>}
  </article>;
}

export function AgentTaskNotifications() {
  const { notices: unreadNotices, storageError } = useTaskNotifications();
  const notices = unreadNotices.filter(notice => !notice.popupClosed);
  const [expanded, setExpanded] = useState(false);
  const tabActivity = useAowTerminalTabActivity();
  const listId = useId();
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => { if (!notices.length) setExpanded(false); }, [notices.length]);

  useEffect(() => {
    const dismissActive = () => {
      const tabId = tabActivity.getCurrent();
      if (tabId) taskNotifications.dismiss(notice => notice.sources.some(source => source.tab_id === tabId));
    };
    dismissActive();
    return tabActivity.subscribe(dismissActive);
  }, [tabActivity]);

  useEffect(() => {
    const source = new EventSource(appUrl('/api/terminals/task-stops'));
    const stopped = (event: MessageEvent<string>) => {
      let notice;
      try { notice = parseTaskStop(JSON.parse(event.data)); } catch { return; }
      if (!notice) return;
      if (notice.sources.some(source => source.tab_id === tabActivity.getCurrent())) return;
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
  return <section className={`agent-task-notifications${expanded ? ' is-expanded' : ''}`} aria-label="任务完成提醒"
    onKeyDown={event => { if (expanded && event.key === 'Escape') { event.stopPropagation(); collapse(); } }}>
    {(notices.length > 1 || expanded) && <div className="agent-task-notifications-toolbar">
      <span>{notices.length} 条待处理</span>
      {expanded && <button onClick={collapse} aria-expanded="true" aria-controls={listId}><ChevronUp size={14} />收起</button>}
      {notices.length > 1 && <button className="agent-task-notifications-clear" title="关闭全部弹框，保留未读提醒" onClick={() => taskNotifications.closePopups(() => true)}>全部关闭</button>}
    </div>}
    {storageError && <div className="agent-task-notifications-storage-error" role="alert">{storageError}</div>}
    <div className="agent-task-notifications-list" id={listId} ref={list} tabIndex={expanded ? 0 : undefined}
      aria-label="待处理通知" aria-live="polite" aria-relevant="additions">
      {visibleNotices.map(notice => <TaskNoticeCard key={notice.key} notice={notice} />)}
    </div>
    {!expanded && foldedCount > 0 && <button className="agent-task-notifications-stack" aria-expanded="false" aria-controls={listId}
      onClick={() => { setExpanded(true); list.current?.scrollTo({ top: 0 }); }}>
      <Layers size={18} aria-hidden="true" /><span><strong>还有 {foldedCount} 条通知</strong><small>展开查看全部</small></span><ChevronDown size={16} aria-hidden="true" />
    </button>}
  </section>;
}
