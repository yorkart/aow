import { t } from '../../lib/i18n';
import { useState } from 'react';
import { useAowTabNavigation } from '../../aow/AowTabEntry';
import { noticeTarget, taskNotifications, type TaskNotice } from './taskNotifications';

export function useTaskNotice(notice: TaskNotice) {
  const navigateTab = useAowTabNavigation();
  const [error, setError] = useState('');
  const [opening, setOpening] = useState(false);
  const sources = notice.sources.filter(source => noticeTarget(source));
  const projectName = [...new Set(sources.map(source => source.project_name || source.workspace_root.split('/').filter(Boolean).pop() || t("notifications.untitled_project")))].join('、')
    || notice.cwd.split('/').filter(Boolean).pop() || t("notifications.untitled_project");
  const tabName = [...new Set(sources.map(source => source.tab_name || t("notifications.untitled_tab")))].join('、') || t("notifications.tab_unavailable");
  const failure = error || (!sources.length ? t("notifications.unable_to_locate_the_corresponding_tab") : '');
  const dismiss = () => taskNotifications.dismiss(item => item.key === notice.key);
  const open = async () => {
    if (opening) return;
    setOpening(true);
    setError('');
    // A session may have several source tabs. Open the first one still available.
    let message = '';
    for (const source of sources) {
      try {
        await navigateTab(noticeTarget(source)!);
        dismiss();
        setOpening(false);
        return;
      } catch (reason) { message = reason instanceof Error ? reason.message : String(reason); }
    }
    setError(message);
    setOpening(false);
  };
  return { projectName, tabName, failure, opening, canOpen: sources.length > 0, open, dismiss };
}
