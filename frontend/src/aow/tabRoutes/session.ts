import { sessionsApi } from '../../features/sessions/api';
import type { AowAgentSession } from '../../features/sessions/types';
import type { TabRouteAdapter, TabTarget } from './types';
import { absolutePath, length, required, segment, url } from './helpers';
export const sessionRoute: TabRouteAdapter<Extract<TabTarget, { type: 'session' }>> = {
  type: 'session',
  centerId: t => 'session:' + t.agent + ':' + t.sessionId,
  parse(parts, q) {
    length(parts, 3);
    const agent = segment(parts, 1) as AowAgentSession['agent'];
    return { type: 'session', workspace: required(q, 'workspace'), agent, sessionId: segment(parts, 2),
      cwd: absolutePath(required(q, 'cwd')), taskId: q.get('task') ?? undefined, runId: q.get('run') ?? undefined };
  },
  url: t => url('session/' + encodeURIComponent(t.agent) + '/' + encodeURIComponent(t.sessionId), {
    workspace: t.workspace, cwd: t.cwd, task: t.taskId, run: t.runId }),
  capture(c) {
    const preview = c.sessions.find(s => c.tab?.id === 'session:' + s.session.id);
    return preview ? { type: 'session', workspace: c.workspace, agent: preview.session.agent, sessionId: preview.session.session_id,
      cwd: preview.workspacePath, taskId: preview.automationRun?.taskId, runId: preview.automationRun?.runId } : undefined;
  },
  async resolve(t) {
    const session = t.taskId && t.runId ? await sessionsApi.automationRunSession(t.taskId, t.runId)
      : (await sessionsApi.agentSessions(t.cwd, t.agent)).find(s => s.session_id === t.sessionId);
    if (!session || session.agent !== t.agent || session.session_id !== t.sessionId) throw new Error('该 Conversation Session 已不存在或无法读取。');
    return { session, target: { ...t, cwd: session.cwd, taskId: undefined, runId: undefined } };
  },
  open: (_, resolved, actions) => actions.session(resolved.session!, resolved.session!.cwd),
};
