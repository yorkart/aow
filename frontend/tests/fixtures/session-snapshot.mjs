export const session = {
  id: 'codex:session-preview', session_id: 'session-preview', agent: 'codex',
  title: '优化 Agent Sessions 会话详情', cwd: '/workspace/aow',
  created_at: '2026-09-15T02:00:00Z', updated_at: '2026-09-15T02:04:00Z',
};

export function makeTurn(id, prompt, final, activities, status = 'completed') {
  return {
    id, status, user: { text: prompt, timestamp: session.created_at },
    final: final ? { text: final, timestamp: session.updated_at } : null,
    activities: activities.map((activity, index) => ({ id: `${id}-${index}`, timestamp: new Date(Date.parse(session.created_at) + index * 10_000).toISOString(), ...activity })),
  };
}

export const snapshot = {
  ...session, captured_at: session.updated_at, status: 'completed', truncated: false,
  turns: [
    makeTurn('turn-1', '梳理一下会话详情页的改进方向。', '建议按轮次组织对话，区分 **处理过程** 和 **最终结论**，方便阅读和回溯。', [
      { kind: 'commentary', text: '正在检查现有会话页面和数据结构。' },
      { kind: 'tool', text: 'exec', status: 'completed' },
      { kind: 'tool', text: 'tool_search', status: 'completed' },
      { kind: 'tool', text: 'exec_command', actions: ['read_files'], status: 'completed' },
    ]),
    makeTurn('turn-2', '历史轮次默认收起过程，最新一轮展开。请按这个规则实施。', '## 已完成调整\n\n- 最新一轮展示处理过程与工具摘要。\n- 历史轮次默认收起，支持手动展开。\n- 刷新时保留当前阅读状态。\n\n```ts\nconst expanded = isLatestTurn;\n```\n\n<img src=x onerror="window.__unsafe=true">', [
      { kind: 'commentary', text: '已定位快照读取逻辑，正在补齐工具调用和进度说明。' },
      { kind: 'tool', text: 'Read', status: 'completed' },
      { kind: 'tool', text: 'exec', status: 'completed' },
      { kind: 'tool', text: 'exec_command', actions: ['read_files', 'search_files'], status: 'completed' },
      { kind: 'tool', text: 'apply_patch', status: 'completed' },
      { kind: 'commentary', text: '页面布局已调整，开始验证历史折叠和最新一轮展开的交互。' },
      { kind: 'tool', text: 'exec', status: 'completed' },
      { kind: 'tool', text: 'exec_command', status: 'failed', details: {
        command: { text: "rg -n 'AgentConfig' crates/server/src/config*", truncated: false },
        cwd: { text: '/workspace/aow', truncated: false },
        output: { text: 'rg: crates/server/src/config*: No such file or directory (os error 2)', truncated: false },
        exit_code: 2, duration_ms: 18,
      } },
      { kind: 'tool', text: 'apply_patch', status: 'completed' },
      { kind: 'tool', text: 'exec_command', status: 'completed' },
    ]),
  ],
};
