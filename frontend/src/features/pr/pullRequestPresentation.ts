export function parsePullRequestNumber(value?: string): number | undefined {
  if (!value || !/^\d+$/.test(value)) return undefined;
  const number = Number(value);
  return Number.isSafeInteger(number) && number > 0 ? number : undefined;
}

export function stateClass(value: string): 'passed' | 'failed' | 'pending' | 'neutral' {
  const state = value.toLowerCase();
  if (['passed', 'succeeded', 'success', 'approved', 'all_passed', 'resolved'].includes(state)) return 'passed';
  if (['failed', 'failure', 'disapproved', 'some_failed', 'timed_out', 'canceled', 'cancelled', 'operation_required', 'action_required', 'startup_failure', 'changes_requested', 'error', 'rejected'].includes(state)) return 'failed';
  if (['pending', 'running', 'in_progress', 'queued', 'warning', 'not_passed', 'unreviewed', 'review_required', 'expected'].includes(state)) return 'pending';
  return 'neutral';
}

export function stateLabel(value: string): string {
  const labels: Record<string, string> = {
    open: '进行中', closed: '已关闭', merged: '已合并', draft: '草稿',
    passed: '已通过', succeeded: '已通过', success: '已通过', approved: '已通过', all_passed: '全部通过',
    failed: '未通过', failure: '未通过', disapproved: '请求修改', rejected: '请求修改', some_failed: '部分未通过',
    pending: '等待中', running: '运行中', in_progress: '运行中', queued: '排队中',
    warning: '有警告', not_passed: '待通过', unreviewed: '待审阅',
    timed_out: '已超时', canceled: '已取消', cancelled: '已取消', operation_required: '需要处理',
    completed: '已完成', skipped: '已跳过', neutral: '无结论', no_checks: '暂无检查',
    changes_requested: '请求修改', review_required: '待审阅', action_required: '需要处理', startup_failure: '启动失败', error: '错误', expected: '等待中', comment: '一般讨论',
    unknown: '状态未知', resolved: '已解决',
  };
  return labels[value.toLowerCase()] || value || '状态未知';
}

export function safeExternalUrl(value?: string | null): string | undefined {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    return ['https:', 'http:'].includes(url.protocol) ? url.href : undefined;
  } catch { return undefined; }
}

export function formatPrTime(value?: string | null): string {
  if (!value) return '未提供';
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return '未提供';
  return new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', hour12: false }).format(date);
}

export function mergeCheckLabel(name: string): string {
  const labels: Record<string, string> = {
    merge_state: '合并状态',
    checkNoConflict: '分支无冲突', checkReviewPassed: '代码审阅', checkCheckRun: 'CI 检查',
    checkCheckRuns: 'CI 检查', checkCheckRunPassed: 'CI 检查', checkAllThreadsResolved: '讨论已解决',
    checkNoUnresolvedThreads: '讨论已解决', checkNotDraft: '已准备好审阅',
    checkSourceBranchExists: '源分支可用', checkTargetBranchExists: '目标分支可用',
    checkFastForward: '可快进合并', checkPullRequestOpen: 'PR 处于开启状态',
  };
  return labels[name] || name;
}
