import { useState } from 'react';
import { useAowTabNavigation } from '../../aow/AowTabEntry';
import { noticeTarget, taskNotifications, type TaskNotice } from './taskNotifications';

export function useTaskNotice(notice: TaskNotice) {
  const navigateTab = useAowTabNavigation();
  const [error, setError] = useState('');
  const [opening, setOpening] = useState(false);
  const sources = notice.sources.filter(source => noticeTarget(source));
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
