import type { Data, SessionSnapshot, ThreadEntry, TurnOverview } from './types';

type EntryPatch = { kind: 'replace'; index: number; entry: ThreadEntry }
  | { kind: 'append_text'; index: number; id: string; text: string };
type TerminalPatch = { kind: 'replace'; id: string; terminal: Data }
  | { kind: 'append_output'; id: string; text: string };
type DialoguePatch = { kind: 'reset'; entries: ThreadEntry[] } | { kind: 'delta'; entries: EntryPatch[] };
type TurnOverviewPatch = Omit<TurnOverview, 'user' | 'messages'> & { user?: ThreadEntry[]; messages?: DialoguePatch };
export type SessionUpdates = { kind: 'snapshot'; snapshot: SessionSnapshot } | { kind: 'unchanged' }
  | { kind: 'overview'; from_revision: number; snapshot: SessionSnapshot; turns: TurnOverviewPatch[] } | {
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
    if (changed.size !== update.turns.length) return undefined;
    const overview: TurnOverview[] = [];
    for (const turn of previous.overview) {
      const patch = changed.get(turn.id);
      changed.delete(turn.id);
      const nextTurn = patch ? applyTurnPatch(turn, patch) : turn;
      if (!nextTurn) return undefined;
      overview.push(nextTurn);
    }
    for (const patch of changed.values()) {
      const nextTurn = applyTurnPatch(undefined, patch);
      if (!nextTurn) return undefined;
      overview.push(nextTurn);
    }
    return { ...next, overview };
  }
  if (previous.thread_id !== update.thread_id || previous.sync_generation !== update.sync_generation) return undefined;
  if (update.revision <= previous.revision) return previous;
  if (previous.revision !== update.from_revision) return undefined;
  const entries = applyEntryPatches(previous.entries, update.entries);
  if (!entries) return undefined;
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

function applyTurnPatch(previous: TurnOverview | undefined, patch: TurnOverviewPatch): TurnOverview | undefined {
  const { messages: change, ...metadata } = patch;
  const user = patch.user ?? previous?.user;
  if (!user) return undefined;
  let messages = previous?.messages;
  if (change?.kind === 'reset') messages = change.entries;
  else if (change?.kind === 'delta') {
    if (!messages) return undefined;
    messages = applyEntryPatches(messages, change.entries);
    if (!messages) return undefined;
  }
  return { ...previous, ...metadata, user, messages };
}

function applyEntryPatches(previous: ThreadEntry[], patches: EntryPatch[]): ThreadEntry[] | undefined {
  const entries = patches.length ? [...previous] : previous;
  for (const patch of patches) {
    if (!Number.isInteger(patch.index) || patch.index < 0 || patch.index > entries.length) return undefined;
    if (patch.kind === 'replace') entries[patch.index] = patch.entry;
    else {
      const entry = entries[patch.index];
      if (!entry || entry.id !== patch.id || typeof entry.content.text !== 'string') return undefined;
      entries[patch.index] = { ...entry, content: { ...entry.content, text: entry.content.text + patch.text } };
    }
  }
  return entries;
}
