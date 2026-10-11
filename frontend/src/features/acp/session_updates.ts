import type { Data, SessionSnapshot, ThreadEntry, TurnOverview } from './types';

type EntryPatch = { kind: 'replace'; index: number; entry: ThreadEntry }
  | { kind: 'append_text'; index: number; id: string; text: string };
type TerminalPatch = { kind: 'replace'; id: string; terminal: Data }
  | { kind: 'append_output'; id: string; text: string };
export type SessionUpdates = { kind: 'snapshot'; snapshot: SessionSnapshot } | { kind: 'unchanged' }
  | { kind: 'overview'; from_revision: number; snapshot: SessionSnapshot; turns: (Omit<TurnOverview, 'user'> & { user?: ThreadEntry[] })[] } | {
  kind: 'delta'; thread_id: string; sync_generation: string; from_revision: number; revision: number; updated_at: string;
  entries: EntryPatch[]; terminals: TerminalPatch[]; fields: Partial<SessionSnapshot>;
};

// Apply atomically to the requested base. Unchanged entries and terminals retain
// their references, allowing memoized renderers to skip the historical content.
// undefined means the cursor cannot be applied and a full resync is required.
export function applySessionUpdates(previous: SessionSnapshot | undefined, update: SessionUpdates): SessionSnapshot | undefined {
  if (update.kind === 'snapshot') return update.snapshot;
  if (!previous || update.kind === 'unchanged') return previous;
  if (update.kind === 'overview') {
    const next = update.snapshot;
    if (!previous.overview || previous.thread_id !== next.thread_id || previous.sync_generation !== next.sync_generation) return undefined;
    if (next.revision <= previous.revision) return previous;
    if (previous.revision !== update.from_revision) return undefined;
    const changed = new Map(update.turns.map(turn => [turn.id, turn]));
    const overview = previous.overview.map(turn => { const patch = changed.get(turn.id); changed.delete(turn.id); return patch ? { ...turn, ...patch } : turn; });
    for (const patch of changed.values()) {
      if (!patch.user) return undefined;
      overview.push({ ...patch, user: patch.user });
    }
    return { ...next, overview };
  }
  if (previous.thread_id !== update.thread_id || previous.sync_generation !== update.sync_generation) return undefined;
  if (update.revision <= previous.revision) return previous;
  if (previous.revision !== update.from_revision) return undefined;
  const entries = update.entries.length ? [...previous.entries] : previous.entries;
  for (const patch of update.entries) {
    if (!Number.isInteger(patch.index) || patch.index < 0 || patch.index > entries.length) return undefined;
    if (patch.kind === 'replace') entries[patch.index] = patch.entry;
    else {
      const entry = entries[patch.index];
      if (!entry || entry.id !== patch.id || typeof entry.content.text !== 'string') return undefined;
      entries[patch.index] = { ...entry, content: { ...entry.content, text: entry.content.text + patch.text } };
    }
  }
  const terminals = update.terminals.length ? { ...previous.terminals } : previous.terminals;
  for (const patch of update.terminals) {
    if (!terminals) return undefined;
    if (patch.kind === 'replace') terminals[patch.id] = patch.terminal;
    else {
      const terminal = terminals[patch.id];
      if (!terminal || typeof terminal.output !== 'string') return undefined;
      terminals[patch.id] = { ...terminal, output: terminal.output + patch.text };
    }
  }
  return { ...previous, ...update.fields, entries, terminals, revision: update.revision, updated_at: update.updated_at };
}
