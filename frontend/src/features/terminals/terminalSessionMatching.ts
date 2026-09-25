import type { TerminalPaneSessions } from './types';
import type { AowAgentSession } from '../sessions/types';

const whitespace = (value: string) => value.replace(/\s+/gu, ' ').trim();
const decoration = /^[\s⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏●✳✶✻✽✢·]+|[\s⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]+$/gu;
const status = /^(?:Ready|Working|Thinking|Waiting|Starting|Action required|Needs input|Codex|TraeCode CLI|Claude Code|Hermes)$/iu;

// Use raw OSC metadata, never the pane's user-defined label. Keep this key
// stable across activity frames so a spinner cannot reset a manual selection.
export function terminalSessionTitle(raw: string, cwd: string) {
  const project = cwd.replace(/\/+$/u, '').split('/').at(-1);
  const parts = whitespace(raw).replace(decoration, '')
    .replace(/^\[ [!.] \] Action Required\s*(?:\|\s*)?/iu, '').split(/\s+\|\s+/u)
    .map(part => part.replace(decoration, '').trim()).filter(Boolean);
  if (parts.at(-1) === project) parts.pop();
  return parts.filter(part => !status.test(part)).join(' | ');
}

export function terminalSessionCandidates(data: TerminalPaneSessions) {
  const title = terminalSessionTitle(data.title, data.cwd);
  const sessions = data.sessions.filter(session => session.agent === data.agent);
  // A native session identity takes precedence even if no transcript has
  // been written yet. Do not silently open an unrelated title match then.
  if (data.live_session_id) {
    const exact = sessions.find(session => session.session_id === data.live_session_id);
    return { title, matches: exact ? [exact] : [], automatic: exact };
  }
  // Hermes' classic CLI has no session OSC title. A leftover shell/agent title
  // cannot identify it when the native resolver has no unique candidate.
  if (data.agent === 'hermes') return { title, matches: [], automatic: undefined };
  const sameDirectory = sessions.filter(session => session.cwd.replace(/\/+$/u, '') === data.cwd.replace(/\/+$/u, ''));
  const exact = title ? sameDirectory.filter(session => whitespace(session.title) === title) : [];
  const prefix = title.replace(/(?:\.\.\.|…)$/u, '');
  const truncated = prefix !== title && Array.from(prefix).length >= 8;
  const matches = truncated
    ? sameDirectory.filter(session => whitespace(session.title).startsWith(prefix)) : exact;
  return { title, matches, automatic: data.process && !truncated && exact.length === 1 ? exact[0] : undefined };
}

export function sessionSearch(sessions: AowAgentSession[], query: string) {
  const needle = whitespace(query).toLocaleLowerCase();
  return sessions.filter(session => `${session.title} ${session.session_id}`.toLocaleLowerCase().includes(needle));
}
