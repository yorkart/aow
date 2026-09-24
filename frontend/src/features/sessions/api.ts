import type { AowAgentSession, AgentSessionSnapshot } from './types';
import { aowRequest } from '../../lib/aowRequest';

export const sessionsApi = {
  agentSessions: (worktreePath: string, agent: AowAgentSession['agent']) => {
    const query = new URLSearchParams({ worktree_path: worktreePath, agent });
    return aowRequest<AowAgentSession[]>(`/api/aow/agent-sessions?${query}`);
  },
  automationRunSession: (taskId: string, runId: string) => {
    const query = new URLSearchParams({ task_id: taskId, run_id: runId });
    return aowRequest<AowAgentSession>(`/api/aow/agent-sessions/automation-run?${query}`);
  },
  agentSessionSnapshot: (session: AowAgentSession, worktreePath: string, signal?: AbortSignal) => {
    const query = new URLSearchParams({ agent: session.agent, worktree_path: worktreePath });
    return aowRequest<AgentSessionSnapshot>(`/api/aow/agent-sessions/${encodeURIComponent(session.session_id)}/snapshot?${query}`, { cache: 'no-store', signal });
  },
};
