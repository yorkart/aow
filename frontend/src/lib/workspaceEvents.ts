import { LiveEvents } from './liveEvents';

interface Snapshot {
  boot_id: string;
  revision: number;
  projects: number;
  terminals: number;
  tasks: number;
  repositories: Record<string, number>;
}

export interface WorkspaceChange {
  reset: boolean;
  projects: boolean;
  terminals: boolean;
  tasks: boolean;
  repositories: ReadonlySet<string>;
}

type Listener = (change: WorkspaceChange) => void;
const listeners = new Set<Listener>();
let source: LiveEvents | undefined;
let fallback: ReturnType<typeof setInterval> | undefined;
let scheduled = false;

function dispatch(change: WorkspaceChange) {
  for (const listener of [...listeners]) listener(change);
}

function reset() {
  dispatch({ reset: true, projects: true, terminals: true, tasks: true, repositories: new Set() });
}

// All registered projects are watched by the server. Changing worktrees does
// not change subscriptions or reconnect this shared browser connection.
function connect() {
  scheduled = false;
  if (source && listeners.size) return;
  source?.close();
  source = undefined;
  clearInterval(fallback);
  fallback = undefined;
  if (!listeners.size) return;
  const connection = new LiveEvents('workspace');
  source = connection;
  let previous: Snapshot | undefined;
  let reconnected = true;
  connection.onopen = () => { reconnected = true; };
  connection.addEventListener('workspace', event => {
    if (source !== connection) return;
    let next: Snapshot;
    try { next = JSON.parse((event as MessageEvent<string>).data); } catch { return; }
    if (!next || typeof next.boot_id !== 'string' || !Number.isSafeInteger(next.revision)
      || !Number.isSafeInteger(next.projects) || !Number.isSafeInteger(next.terminals)
      || !next.repositories || typeof next.repositories !== 'object') return;
    if (previous?.boot_id === next.boot_id && previous.revision > next.revision) return;
    clearInterval(fallback);
    fallback = undefined;
    const changed: WorkspaceChange = {
      reset: reconnected || !previous || previous.boot_id !== next.boot_id,
      projects: previous?.projects !== next.projects,
      terminals: previous?.terminals !== next.terminals,
      tasks: previous?.tasks !== next.tasks,
      repositories: new Set(Object.keys(next.repositories).filter(root => previous?.repositories[root] !== next.repositories[root])),
    };
    previous = next;
    reconnected = false;
    dispatch(changed);
  });
  connection.onerror = () => {
    if (source !== connection || fallback) return;
    // Only degraded connections poll. Healthy connections never poll history.
    fallback = setInterval(() => { if (document.visibilityState === 'visible') reset(); }, 30_000);
  };
}

function scheduleConnection() {
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(connect);
}

export function subscribeWorkspaceChanges(listener: Listener): () => void {
  listeners.add(listener);
  scheduleConnection();
  return () => { listeners.delete(listener); scheduleConnection(); };
}
