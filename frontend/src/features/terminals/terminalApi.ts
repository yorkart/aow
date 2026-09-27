import { appUrl } from '../../lib/basePath';
import { defaultTerminalPaneName } from './terminalPresentation';
import type { TerminalAgentList, TerminalLayout, TerminalPane, TerminalPaneStatus, TerminalPaneSessions, TerminalAgentProcess, TerminalSplitAxis, TerminalTab } from './types';

const TERMINALS_PATH = appUrl('/api/terminals');
const paneStatuses = new Set<TerminalPaneStatus>(['running', 'exited', 'interrupted']);

class TerminalHttpError extends Error {
  constructor(message: string, readonly status: number) { super(message); }
}

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined;
}

function errorMessage(payload: unknown, fallback: string) {
  const value = asRecord(payload);
  const error = asRecord(value?.error);
  return typeof value?.message === 'string'
    ? value.message
    : typeof error?.message === 'string'
      ? error.message
      : fallback;
}

async function request(url: string, init?: RequestInit, timeoutMs = 15_000, externalSignal?: AbortSignal): Promise<unknown> {
  const controller = new AbortController();
  let timedOut = false;
  const abortFromExternal = () => controller.abort();
  if (externalSignal?.aborted) controller.abort();
  else externalSignal?.addEventListener('abort', abortFromExternal, { once: true });
  const timer = window.setTimeout(() => { timedOut = true; controller.abort(); }, timeoutMs);
  try {
    const response = await fetch(url, {
      ...init,
      signal: controller.signal,
      headers: init?.body === undefined
        ? init?.headers
        : { 'Content-Type': 'application/json', ...init.headers },
    });
    const payload = response.status === 204 ? null : await response.json().catch(() => null);
    if (!response.ok) throw new TerminalHttpError(errorMessage(payload, `HTTP ${response.status}`), response.status);
    return payload;
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') {
      if (timedOut) throw new Error(`终端请求超时（${Math.round(timeoutMs / 1000)} 秒）`);
    }
    throw error;
  } finally {
    window.clearTimeout(timer);
    externalSignal?.removeEventListener('abort', abortFromExternal);
  }
}

function normalizeLayout(value: unknown): TerminalLayout | null {
  const item = asRecord(value);
  if (!item) return null;
  if (item.type === 'pane' && typeof item.pane_id === 'string') {
    return { type: 'pane', pane_id: item.pane_id };
  }
  if (item.type !== 'split' || (item.axis !== 'row' && item.axis !== 'column')) return null;
  const first = normalizeLayout(item.first);
  const second = normalizeLayout(item.second);
  if (!first || !second) return null;
  const ratio = typeof item.ratio === 'number' && Number.isFinite(item.ratio)
    ? Math.min(.9, Math.max(.1, item.ratio))
    : .5;
  return { type: 'split', axis: item.axis, ratio, first, second };
}

function normalizePane(value: unknown): TerminalPane | null {
  const item = asRecord(value);
  if (!item || typeof item.id !== 'string') return null;
  const status = paneStatuses.has(item.status as TerminalPaneStatus)
    ? item.status as TerminalPaneStatus
    : 'running';
  const cwd = typeof item.cwd === 'string' ? item.cwd : '';
  const shell = typeof item.shell === 'string' ? item.shell : '';
  const agentTerminal = asRecord(item.agent_terminal);
  return {
    id: item.id,
    // Pane names are generated; ignore legacy custom names from stored metadata
    // and older servers, including when no OSC title is available.
    name: defaultTerminalPaneName({ cwd, shell }),
    cwd,
    shell,
    arguments: Array.isArray(item.arguments) ? item.arguments.filter((value): value is string => typeof value === 'string') : [],
    kind: item.kind === 'agent' ? 'agent' : 'terminal',
    ...(agentTerminal && ['starting', 'ready', 'failed'].includes(String(agentTerminal.phase)) ? {
      agent_terminal: {
        phase: agentTerminal.phase as 'starting' | 'ready' | 'failed',
        error: typeof agentTerminal.error === 'string' ? agentTerminal.error : null,
        task_submitted: agentTerminal.task_submitted === true,
      },
    } : {}),
    ...(typeof item.agent_id === 'string' || item.agent_id === null
      ? { agent_id: item.agent_id } : {}),
    ...(typeof item.restart_on_daemon_restart === 'boolean' ? { restart_on_daemon_restart: item.restart_on_daemon_restart } : {}),
    status,
    ...(typeof item.exit_code === 'number' || item.exit_code === null
      ? { exit_code: item.exit_code }
      : {}),
    ...(typeof item.rows === 'number' ? { rows: item.rows } : {}),
    ...(typeof item.cols === 'number' ? { cols: item.cols } : {}),
  };
}

function normalizeTab(value: unknown): TerminalTab | null {
  const item = asRecord(value);
  if (!item || typeof item.id !== 'string') return null;
  return {
    id: item.id,
    name: typeof item.name === 'string' && item.name.trim() ? item.name : 'Terminal',
    ...(typeof item.name_is_custom === 'boolean' ? { name_is_custom: item.name_is_custom } : {}),
    workspace_root: typeof item.workspace_root === 'string' ? item.workspace_root : '',
    layout: normalizeLayout(item.layout),
    panes: Array.isArray(item.panes)
      ? item.panes.map(normalizePane).filter((pane): pane is TerminalPane => pane !== null)
      : [],
    ...(typeof item.revision === 'number' || typeof item.revision === 'string'
      ? { revision: item.revision }
      : {}),
    ...(typeof item.created_at === 'string' || typeof item.created_at === 'number' ? { created_at: item.created_at } : {}),
    ...(typeof item.updated_at === 'string' || typeof item.updated_at === 'number' ? { updated_at: item.updated_at } : {}),
  };
}

export function tabFromPayload(payload: unknown): TerminalTab | null {
  const direct = normalizeTab(payload);
  if (direct) return direct;
  const envelope = asRecord(payload);
  return normalizeTab(envelope?.tab ?? envelope?.terminal);
}

function terminalPath(tabId: string, suffix = '') {
  return `${TERMINALS_PATH}/${encodeURIComponent(tabId)}${suffix}`;
}

export const terminalApi = {
  async agents(workspaceRoot?: string, signal?: AbortSignal): Promise<TerminalAgentList> {
    const query = new URLSearchParams(workspaceRoot === undefined ? {} : { workspace_root: workspaceRoot });
    const payload = asRecord(await request(`${TERMINALS_PATH}/agents?${query}`, undefined, 5_000, signal));
    const agents = asRecord(payload?.agents);
    if (!agents) throw new Error('服务器返回了无效的终端 Agent 数据');
    return {
      agents: Object.fromEntries(Object.entries(agents).filter((entry): entry is [string, string | null] =>
        entry[1] === null || typeof entry[1] === 'string' && entry[1].length > 0)),
      titles: Object.fromEntries(Object.entries(asRecord(payload?.titles) ?? {})
        .filter((entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1].trim().length > 0)),
      processes: Object.fromEntries(Object.entries(asRecord(payload?.processes) ?? {}).flatMap(([id, value]) => {
        const process = asRecord(value);
        return process && Number.isInteger(process.pid) && (process.pid as number) > 0
          && typeof process.start_time === 'string' && typeof process.cwd === 'string'
          ? [[id, process as unknown as TerminalAgentProcess]] : [];
      })),
    };
  },

  async paneSessions(tabId: string, paneId: string, signal?: AbortSignal): Promise<TerminalPaneSessions> {
    return await request(terminalPath(tabId, `/panes/${encodeURIComponent(paneId)}/agent-sessions`),
      { cache: 'no-store' }, 15_000, signal) as TerminalPaneSessions;
  },

  async list(workspaceRoot?: string, signal?: AbortSignal): Promise<TerminalTab[]> {
    const query = new URLSearchParams(workspaceRoot === undefined ? {} : { workspace_root: workspaceRoot });
    const payload = await request(`${TERMINALS_PATH}?${query}`, undefined, 15_000, signal);
    const envelope = asRecord(payload);
    const values = Array.isArray(payload) ? payload : Array.isArray(envelope?.tabs) ? envelope.tabs : [];
    return values.map(normalizeTab).filter((tab): tab is TerminalTab => tab !== null);
  },

  async get(tabId: string, signal?: AbortSignal): Promise<TerminalTab> {
    try {
      const tab = tabFromPayload(await request(terminalPath(tabId), undefined, 15_000, signal));
      if (!tab) throw new Error('服务器返回了无效的终端数据');
      return tab;
    } catch (reason) {
      if (reason instanceof TerminalHttpError && reason.status === 404) throw new Error('该 Tab 已关闭或不存在。');
      throw reason;
    }
  },

  async create(workspaceRoot: string, options: { cwd: string; name?: string; agentId?: string; resumeSessionId?: string }): Promise<TerminalTab> {
    const payload = await request(TERMINALS_PATH, {
      method: 'POST',
      body: JSON.stringify({
        workspace_root: workspaceRoot,
        cwd: options.cwd,
        ...(options.name?.trim() ? { name: options.name.trim() } : {}),
        ...(options.agentId ? { agent_id: options.agentId } : {}),
        ...(options.resumeSessionId !== undefined ? { resume_session_id: options.resumeSessionId } : {}),
      }),
    });
    const tab = tabFromPayload(payload);
    if (!tab) throw new Error('服务器返回了无效的终端数据');
    return tab;
  },

  async rename(tabId: string, name: string): Promise<TerminalTab | null> {
    return tabFromPayload(await request(terminalPath(tabId), {
      method: 'PATCH',
      body: JSON.stringify({ name }),
    }));
  },

  async reorder(workspaceRoot: string, tabIds: string[]): Promise<TerminalTab[]> {
    const payload = await request(`${TERMINALS_PATH}/order`, {
      method: 'PUT',
      body: JSON.stringify({ workspace_root: workspaceRoot, tab_ids: tabIds }),
    });
    const envelope = asRecord(payload);
    const values = Array.isArray(payload) ? payload : Array.isArray(envelope?.tabs) ? envelope.tabs : [];
    return values.map(normalizeTab).filter((tab): tab is TerminalTab => tab !== null);
  },

  close(tabId: string): Promise<unknown> {
    return request(terminalPath(tabId), { method: 'DELETE' });
  },

  async rebuild(tabId: string): Promise<TerminalTab> {
    const tab = tabFromPayload(await request(terminalPath(tabId, '/rebuild'), { method: 'POST' }));
    if (!tab) throw new Error('服务器返回了无效的终端数据');
    return tab;
  },

  async split(
    tabId: string,
    targetPaneId: string,
    axis: TerminalSplitAxis,
    options?: { ratio?: number; cwd?: string; shell?: string },
  ): Promise<TerminalTab | null> {
    return tabFromPayload(await request(terminalPath(tabId, '/split'), {
      method: 'POST',
      body: JSON.stringify({
        target_pane_id: targetPaneId,
        axis,
        ...(options?.ratio === undefined ? {} : { ratio: options.ratio }),
        ...(options?.cwd ? { cwd: options.cwd } : {}),
        ...(options?.shell ? { shell: options.shell } : {}),
      }),
    }));
  },

  async closePane(tabId: string, paneId: string): Promise<TerminalTab | null> {
    const suffix = `/panes/${encodeURIComponent(paneId)}`;
    return tabFromPayload(await request(terminalPath(tabId, suffix), { method: 'DELETE' }));
  },

  async uploadClipboardImage(tabId: string, paneId: string, file: File, signal?: AbortSignal): Promise<{ path: string }> {
    const suffix = `/panes/${encodeURIComponent(paneId)}/clipboard-images`;
    const payload = await request(terminalPath(tabId, suffix), {
      method: 'POST',
      body: file,
      headers: { 'Content-Type': file.type || 'application/octet-stream' },
    }, 30_000, signal);
    const response = asRecord(payload);
    if (typeof response?.path !== 'string' || response.path.length === 0) {
      throw new Error('服务器返回了无效的图片路径');
    }
    return { path: response.path };
  },

  async updateLayout(tabId: string, layout: TerminalLayout, revision?: number | string): Promise<TerminalTab | null> {
    return tabFromPayload(await request(terminalPath(tabId, '/layout'), {
      method: 'PUT',
      body: JSON.stringify({ layout, ...(revision === undefined ? {} : { revision }) }),
    }));
  },

  webSocketUrl(tabId: string, paneId: string, resume?: { epoch: string; after: number }) {
    const url = new URL(terminalPath(tabId, `/panes/${encodeURIComponent(paneId)}/ws`), window.location.href);
    url.protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    url.searchParams.set('control', 'v2');
    // Full VT restores must only be sent to clients that understand the
    // accompanying snapshot geometry. Unknown query parameters are ignored
    // by older servers, which keeps rolling upgrades on the raw replay path.
    url.searchParams.set('capabilities', 'vt-snapshot-v1');
    // Ask newer terminald instances for a read-only stream when another
    // browser attachment already owns terminal input. Older servers ignore
    // this capability and retain their existing waiting behavior.
    url.searchParams.set('observer', 'v1');
    if (resume !== undefined) {
      if (!resume.epoch) throw new RangeError('终端续传 epoch 不能为空');
      if (!Number.isSafeInteger(resume.after) || resume.after < 0) throw new RangeError('终端续传位置必须是非负安全整数');
      url.searchParams.set('epoch', resume.epoch);
      url.searchParams.set('after', String(resume.after));
    }
    return url.toString();
  },

  resizeMessage(cols: number, rows: number) {
    return JSON.stringify({ type: 'resize', cols, rows });
  },

  claimMessage(force: boolean) {
    return JSON.stringify({ type: 'claim', force });
  },
};
