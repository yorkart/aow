// Representative test data, never a substitute for a live PR response.
const author = { id: 'author', username: 'chen.yu', display_name: '陈宇' };
const reviewer = { id: 'reviewer', username: 'lin.xiao', display_name: '林晓' };
const comment = (id, author, body) => ({ id, author, body, created_at: '2026-09-12T02:20:00Z', updated_at: '2026-09-12T02:20:00Z' });
const threads = [
  { id: 't1', path: 'frontend/src/features/pr/PullRequestDetailView.tsx', line: 128, status: 'open', author: '陈宇', body: '已补充异常提示。', updated_at: '2026-09-12T03:20:00Z', comments: [comment('c1', '林晓', '这里的加载失败需要保留已有数据，避免用户丢失当前阅读位置。\n\n建议保留 **上一次成功加载** 的内容。'), comment('c2', '陈宇', '已补充异常提示，刷新失败会保留已有内容。')] },
  { id: 't2', path: null, line: null, status: 'resolved', author: '林晓', body: '类型检查通过。', updated_at: '2026-09-12T04:20:00Z', comments: [comment('c3', '林晓', '类型检查通过。\n\n- [x] 描述中的清单只读\n- [x] 代码变更只读')] },
];
export const detail = {
  number: 42, title: 'feat: 优化 Pull Request 详情展示与只读浏览体验', status: 'open', draft: false,
  source_branch: 'feature/pr-details', target_branch: 'develop', url: 'https://github.com/example/aow/pull/42',
  created_at: '2026-09-11T06:32:00Z', updated_at: '2026-09-12T03:45:00Z',
  description: '## 背景\n\n在工作台中快速了解 PR 的当前进展，让代码变更、审阅反馈和检查结果更容易阅读。\n\n## 变更内容\n\n- 增加作者、审阅人与变更统计\n- 展示完整讨论与 CI 检查输出\n- 支持文件筛选和两种 Diff 布局\n\n### 验证\n\n- [x] TypeScript 类型检查\n- [x] Rust 解析回归测试\n- [ ] 浏览器视觉检查\n\n```ts\nconst options = { readOnly: true, originalEditable: false };\n```\n\n> 此页面仅用于查看 PR 信息。',
  changes_count: 4, commits_count: 3, diverged_commits_count: 2, milestone: 'AoW · September', review_status: 'pending', check_summary_status: 'some_failed', mergeable: false,
  author, reviewers: [reviewer, { id: 'r2', username: 'zhou.ming', display_name: '周明' }], labels: ['frontend', '体验优化'],
  files: [{ path: 'frontend/src/features/pr/PullRequestDetailView.tsx', change_type: 'modified', additions: 186, deletions: 74 }, { path: 'frontend/src/features/pr/PullRequestDetailView.css', change_type: 'added', additions: 128, deletions: 0 }, { path: 'crates/server/src/pull_requests.rs', change_type: 'modified', additions: 62, deletions: 18 }, { path: 'assets/preview.png', change_type: 'added', additions: null, deletions: null }],
  checks: [
    { id: 'build', name: 'Frontend build', status: 'completed', conclusion: 'succeeded', description: 'TypeScript 类型检查与生产构建', text: '### Build summary\n\n✅ TypeScript\n\n✅ Vite production build', details_url: 'https://example.com/check/build', required: true, started_at: '2026-09-12T03:20:00Z', completed_at: '2026-09-12T03:22:00Z' },
    { id: 'test', name: 'Rust unit tests', status: 'completed', conclusion: 'failed', description: '单元测试中有 1 项未通过', text: '### Test result\n\n❌ `retains_discussion_authors`\n\n```text\nexpected: reviewer\nreceived: empty string\n```', details_url: 'https://example.com/check/test', required: true },
    { id: 'lint', name: 'Code quality', status: 'in_progress', conclusion: '', description: '正在分析变更文件', text: '', details_url: null, required: false },
  ],
  threads, unresolved_threads: [threads[0]], warnings: [],
  merge_checks: [{ name: 'checkNoConflict', passed: true, reason: '' }, { name: 'checkReviewPassed', passed: false, reason: '需要至少 1 位审阅人通过。' }, { name: 'checkCheckRun', passed: false, reason: 'Rust unit tests 尚未通过，请查看检查详情。' }],
};
