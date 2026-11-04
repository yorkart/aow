import type { Task } from './model';

export type Session = {
  id: string; title: string; kind: 'acp' | 'terminal'; agentId: string; parentId: string;
  dependsOn: string[]; status: 'running' | 'waiting' | 'done' | 'paused'; ticks: number;
  stage: 'execute' | 'review' | 'approval' | 'auxiliary'; goal: string; draft: string;
  messages: string[]; output: string[];
};
export type WorkspaceTab = { id: string; kind: 'main' | 'session' | 'file' | 'properties' | 'workflow'; target: string; title: string };
export type TaskDocument = { path: string; content: string; saved: string };
export type TaskProperties = {
  labels: string; parentId: string; blockedBy: string;
  reviewer: string; approver: string; monitor: string; watchdog: string;
  created: string; updated: string; origin: string;
};
export type TaskWorkspaceState = {
  tabs: WorkspaceTab[]; activeTab: string; sessions: Session[]; documents: TaskDocument[];
  properties: TaskProperties; phase: 'execute' | 'review' | 'approval' | 'done'; approved: boolean;
};

export const sessionLabels: Record<Session['status'], string> = { running: '执行中', waiting: '等待依赖', done: '已完成', paused: '已暂停' };
export const mainTab: WorkspaceTab = { id: 'main', kind: 'main', target: 'main', title: '主会话' };

export function newSession(values: Pick<Session, 'id' | 'title' | 'agentId'> & Partial<Session>): Session {
  return { kind: 'acp', parentId: '', dependsOn: [], status: 'running', ticks: 0, stage: 'auxiliary', goal: '', draft: '', messages: [], output: [], ...values };
}

export function prepareTask(task: Task): Task {
  if (task.workspace) return task;
  const showcase = task.id === 'AOW-128';
  const finished = task.status === 'done' || task.status === 'review';
  const main = newSession({ id: 'main', title: task.title, agentId: task.agentId, stage: 'execute', goal: task.prompt,
    status: showcase ? 'waiting' : finished ? 'done' : task.status === 'paused' ? 'paused' : 'running',
    dependsOn: showcase ? ['design', 'implementation', 'checks'] : [] });
  const sessions = [main];
  if (showcase) sessions.push(
    newSession({ id: 'design', title: '交互方案', agentId: 'designer', parentId: 'main', stage: 'execute', status: 'done', goal: '梳理任务工作区、Tab 和 Properties 的交互。', messages: ['方案已整理：主会话固定，辅助会话和文件使用独立 Tab。'] }),
    newSession({ id: 'implementation', title: '实现任务 Tabs', agentId: 'engineer', parentId: 'main', stage: 'execute', dependsOn: ['design'], goal: '实现任务级 Tab 栏，并保留每个会话的上下文。' }),
    newSession({ id: 'checks', title: '验证 · Terminal', agentId: '', kind: 'terminal', parentId: 'implementation', stage: 'execute', status: 'waiting', dependsOn: ['implementation'], goal: '等待实现完成后验证构建和类型检查。', output: ['$ npm run typecheck', '等待「实现任务 Tabs」完成后开始模拟验证。'] }),
  );
  const now = new Date().toISOString();
  return { ...task, workspace: {
    tabs: showcase ? [mainTab, { id: 'session:implementation', kind: 'session', target: 'implementation', title: '实现任务 Tabs' }, { id: 'session:checks', kind: 'session', target: 'checks', title: '验证 · Terminal' }] : [mainTab],
    activeTab: 'main', sessions, documents: [], phase: task.status === 'review' ? 'review' : finished ? 'done' : 'execute', approved: finished,
    properties: { labels: showcase ? '产品设计, frontend' : '', parentId: '', blockedBy: '', reviewer: showcase ? 'designer' : task.status === 'review' ? 'researcher' : '', approver: showcase ? 'researcher' : '', monitor: showcase ? 'researcher' : '', watchdog: '', created: now, updated: now, origin: task.prompt.startsWith('自动化任务') ? 'Automation' : '手动创建' },
  } };
}

export function openWorkspaceTab(workspace: TaskWorkspaceState, tab: WorkspaceTab): TaskWorkspaceState {
  return { ...workspace, tabs: workspace.tabs.some(item => item.id === tab.id) ? workspace.tabs : [...workspace.tabs, tab], activeTab: tab.id };
}

export function openSession(workspace: TaskWorkspaceState, id: string) {
  const session = workspace.sessions.find(item => item.id === id);
  if (!session) return workspace;
  return openWorkspaceTab(workspace, id === 'main' ? mainTab : { id: `session:${id}`, kind: 'session', target: id, title: session.title });
}

export function closeWorkspaceTab(workspace: TaskWorkspaceState, id: string): TaskWorkspaceState {
  if (id === 'main') return workspace;
  const index = workspace.tabs.findIndex(item => item.id === id);
  const tabs = workspace.tabs.filter(item => item.id !== id);
  return { ...workspace, tabs, activeTab: workspace.activeTab === id ? tabs[Math.max(0, index - 1)]?.id ?? 'main' : workspace.activeTab };
}

export function repositoryFixture(projectId: string) {
  if (projectId === 'paperclip') return { root: 'ui', directory: 'components', files: ['Sidebar.tsx', 'CompanySwitcher.tsx', 'sidebar.css'] };
  if (projectId === 'aow') return { root: 'frontend', directory: 'components', files: ['TaskWorkspace.tsx', 'TaskComposer.tsx', 'workspace.css'] };
  return { root: 'app', directory: 'tasks', files: ['task.ts', 'task-view.tsx', 'task.css'] };
}

export function fixturePaths(task: Task, repository: boolean) {
  const fixture = repositoryFixture(task.projectId);
  return repository ? [...fixture.files.map(name => `${fixture.root}/src/${fixture.directory}/${name}`), 'package.json', 'README.md', '.gitignore'] : ['task.md', 'result.md'];
}

export function openDocument(task: Task, path: string, create = false): Task {
  const workspace = task.workspace!;
  const content = create ? '' : path.endsWith('.md') ? `# ${task.title}\n\n${task.prompt}\n\n## 工作上下文\n\nTask: ${task.id}\ncwd: ${task.cwd}\n\n主会话负责汇总，子会话协作完成任务。\n` : path.endsWith('.json') ? '{\n  "name": "task-workspace-demo",\n  "private": true\n}\n' : `// ${path} · 演示文件\n// Task: ${task.id}\n\nconst workspace = {\n  task: '${task.id}',\n  cwd: '${task.cwd}',\n  primarySession: 'main',\n  tabs: ['conversation', 'terminal', 'files'],\n};\n\nexport default workspace;\n`;
  const documents = workspace.documents.some(item => item.path === path) ? workspace.documents : [...workspace.documents, { path, content, saved: content }];
  return { ...task, workspace: openWorkspaceTab({ ...workspace, documents }, { id: `file:${path}`, kind: 'file', target: path, title: path.split('/').pop()! }) };
}

// Parentage records delegation. Only explicit dependsOn edges block execution.
// Children can finish after their parent (for example its terminal verification).
export function advanceTask(task: Task, allTasks: Task[]): Task {
  const workspace = task.workspace!;
  if (task.status === 'paused' || workspace.properties.blockedBy && allTasks.some(item => item.id === workspace.properties.blockedBy && item.status !== 'done')) return task;
  const sessions = workspace.sessions.map(session => {
    if (session.status === 'done' || session.status === 'paused' || session.stage === 'approval') return session;
    // An interactive terminal remains available after a task completes. Only
    // delegated verification terminals represent a finite simulated step.
    if (session.kind === 'terminal' && session.stage === 'auxiliary') return session;
    if (session.dependsOn.some(id => workspace.sessions.find(item => item.id === id)?.status !== 'done')) return { ...session, status: 'waiting' as const };
    const ticks = session.ticks + 1;
    return { ...session, ticks, status: ticks >= 3 ? 'done' as const : 'running' as const };
  });
  let phase = workspace.phase;
  const { reviewer, approver } = workspace.properties;
  if (phase === 'execute' && sessions.filter(item => item.stage === 'execute').every(item => item.status === 'done')) {
    phase = reviewer ? 'review' : approver ? 'approval' : 'done';
  }
  if (phase === 'review') {
    const review = sessions.find(item => item.stage === 'review');
    if (!reviewer || review?.status === 'done') phase = approver ? 'approval' : 'done';
    else if (!review) sessions.push(newSession({ id: `workflow-review-${sessions.length}`, title: 'Review · 结果审查', agentId: reviewer, parentId: 'main', stage: 'review', goal: '审查主会话汇总的任务结果，检查边界与遗漏。' }));
  }
  if (phase === 'approval') {
    if (!approver || workspace.approved) phase = 'done';
    else if (!sessions.some(item => item.stage === 'approval')) sessions.push(newSession({ id: `workflow-approval-${sessions.length}`, title: 'Approval · 执行批准', agentId: approver, parentId: 'main', stage: 'approval', status: 'waiting', goal: '检查审查结果，给出本次任务的批准决定。' }));
  }
  const status = phase === 'done' ? 'done' : phase === 'execute' ? 'running' : 'review';
  const work = sessions.filter(item => item.stage === 'execute');
  const progress = Math.min(3, Math.floor(work.filter(item => item.status === 'done').length / work.length * 3));
  return { ...task, status, progress, workspace: { ...workspace, phase, sessions } };
}

export function relationshipCandidates(task: Task, tasks: Task[], field: 'parentId' | 'blockedBy') {
  return tasks.filter(candidate => {
    const visited = new Set<string>([task.id]);
    let current: Task | undefined = candidate;
    while (current) {
      if (visited.has(current.id)) return false;
      visited.add(current.id);
      const next: string | undefined = current.workspace?.properties[field];
      current = next ? tasks.find(item => item.id === next) : undefined;
    }
    return true;
  });
}
