import type { AutomationAgent, WorkspaceMode, RunStatus } from './types';
export const agentNames: Record<AutomationAgent, string> = { codex: 'Codex', traecli: 'TraeCode CLI', claude: 'Claude Code' };
export const workspaceNames: Record<WorkspaceMode, string> = { existing: '现有工作区', new_worktree: '新建 Worktree', new_branch: '新建分支', temporary: '临时目录（TMP）' };
export const runNames: Record<RunStatus, string> = { preparing: '准备中', running: '执行中', completed: '已完成', failed: '失败', skipped: '已跳过', interrupted: '已中断' };
export const errorMessage = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
export function dateTime(value: string | null) {
  return value ? new Date(value).toLocaleString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }) : '—';
}
export function duration(ms: number | null) {
  if (ms === null) return '—';
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} 秒`;
  const totalSeconds = Math.floor(ms / 1000);
  const seconds = totalSeconds % 60;
  const totalMinutes = Math.floor(totalSeconds / 60);
  const minutes = totalMinutes % 60;
  const hours = Math.floor(totalMinutes / 60);
  return hours > 0
    ? `${hours} 时 ${minutes} 分 ${seconds} 秒`
    : `${minutes} 分 ${seconds} 秒`;
}
export function scheduleName(cron: string, seconds?: number | null) {
  if (seconds) {
    const unit = seconds % 3600 === 0 ? 3600 : seconds % 60 === 0 ? 60 : 1;
    return `每 ${seconds / unit} ${unit === 3600 ? '小时' : unit === 60 ? '分钟' : '秒'}`;
  }
  const [minute, hour, day, month, weekday] = cron.trim().split(/\s+/);
  if (day === '*' && month === '*' && /^\d+$/.test(minute)) {
    if (hour === '*' && weekday === '*') return `每小时 :${minute.padStart(2, '0')}`;
    if (/^\d+$/.test(hour)) {
      const time = `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}`;
      if (weekday === '*') return `每天 ${time}`;
      if (weekday === '1-5') return `工作日 ${time}`;
      if (/^[0-7]$/.test(weekday)) return `每周${'日一二三四五六日'[Number(weekday)]} ${time}`;
    }
  }
  return cron;
}
