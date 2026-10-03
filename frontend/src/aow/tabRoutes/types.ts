import type { AutomationTask } from '../../features/automations/types';
import type { WorkspaceTab } from '../WorkspaceTabs';
import type { DocumentSource, OpenDocument } from '../../features/editor/types';
import type { PullRequestSummary } from '../../features/pr/types';
import type { TerminalTab } from '../../features/terminals/types';
import type { AowAgentSession } from '../../features/sessions/types';
import type { AowProject, AowWorktree } from '../types';

export type TabTarget =
  | { type: 'terminal'; tabId: string }
  | { type: 'files'; workspace: string; path: string }
  | { type: 'file'; workspace: string; path: string; source: DocumentSource }
  | { type: 'diff'; workspace: string; repository: string; path: string; source: 'working' | 'staged' | 'commit'; commit?: string; originalPath?: string; untracked?: boolean }
  | { type: 'pr'; workspace: string; repository: string; provider: string; remote?: string; number: number }
  | { type: 'session'; workspace: string; agent: AowAgentSession['agent']; sessionId: string; cwd: string; taskId?: string; runId?: string }
  | { type: 'automation'; workspace: string; taskId: string; runId?: string; view?: 'runs' };
export interface AutomationLocation { view: 'overview' | 'runs'; runId?: string }
export interface TabContext {
  workspace: string; workspacePath: string; tab?: WorkspaceTab;
  documents: OpenDocument[];
  sessions: { session: AowAgentSession; workspacePath: string; automationRun?: { taskId: string; runId: string } }[];
  pullRequests: (PullRequestSummary & { repository?: string })[];
  browserPath: string;
  automationLocations: Record<string, AutomationLocation>;
}
export interface ResolvedTab {
  target: TabTarget; workspacePath: string;
  terminal?: TerminalTab; session?: AowAgentSession;
  pullRequest?: PullRequestSummary; task?: AutomationTask;
}
export interface TabOpenActions {
  terminal: (id: string) => void;
  files: (path: string) => void;
  file: (path: string, source: DocumentSource) => void | Promise<void>;
  diff: (target: Extract<TabTarget, { type: 'diff' }>) => void | Promise<void>;
  pullRequest: (pr: PullRequestSummary, repository: string) => void;
  session: (session: AowAgentSession, cwd: string) => void;
  automation: (task: AutomationTask, location: AutomationLocation) => void;
}
export interface ResolveContext { project: AowProject; worktree: AowWorktree }
export interface TabRouteAdapter<T extends TabTarget = TabTarget> {
  type: T['type'];
  centerId: (target: T) => string;
  parse: (segments: string[], query: URLSearchParams) => T;
  url: (target: T) => string;
  capture: (context: TabContext) => T | undefined;
  resolve: (target: T, context: ResolveContext, signal: AbortSignal) => Promise<Partial<ResolvedTab>>;
  open: (target: T, resolved: ResolvedTab, actions: TabOpenActions) => void | Promise<void>;
}
