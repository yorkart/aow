import { t } from '../../../../lib/i18n';
import { contentMarkdown, messageGroups, visibleContent, type MessageGroup } from './entry_view_state';
import { object, text, type Data, type SessionSnapshot, type SessionTurn, type ThreadEntry, type TurnOverview } from '../../types';
import { userTextPreview } from './user_text_images';
import { coalesceSteeringChunks, isSteeringDialogue } from './steering_messages';

function dialogueReply(entries: ThreadEntry[]) {
  return coalesceSteeringChunks(entries).filter(entry => entry.kind === 'assistant')
    .map(entry => contentMarkdown(entry.content)).filter(Boolean).join('\n\n');
}

export function overviewPresentation(turn: TurnOverview): ReturnType<typeof turnPresentation> {
  return {
    running: turn.running, waiting: turn.waiting, status: turn.status ?? undefined,
    process: [], conclusion: messageGroups(turn.conclusion, false, false),
    messages: turn.messages ? messageGroups(turn.messages, turn.running, turn.waiting) : undefined,
    reply: dialogueReply(turn.messages ?? turn.conclusion),
    error: turn.error,
  };
}

export function overviewPrompt(turn: TurnOverview) {
  return turn.user.map(entry => promptText(entry.content)).filter(Boolean).join('\n');
}

function promptText(content: Data): string {
  if (content.type === 'text') return userTextPreview(text(content.text));
  if (content.type === 'image') return t("acp.image");
  if (content.type === 'resource') return text(object(content.resource).text) || text(object(content.resource).uri);
  return text(content.name) || text(content.uri);
}

export function conversationTurns(groups: MessageGroup[]) {
  const leading: MessageGroup[] = [];
  const turns: { id: string; prompt: string; user: MessageGroup; response: MessageGroup[] }[] = [];
  for (const group of groups) {
    if (group.kind === 'user') {
      turns.push({ id: group.id, prompt: group.entries.map(entry => promptText(entry.content)).filter(Boolean).join('\n'), user: group, response: [] });
    } else {
      const turn = turns.at(-1);
      if (turn) turn.response.push(group); else leading.push(group);
    }
  }
  return { leading, turns };
}

// ACP has no final-message flag. Only a trailing complete agent message is a
// candidate; a later tool/thought must never promote earlier progress to a final.
export function finalMessageEntries(entries: ThreadEntry[]): ThreadEntry[] {
  const relevant = entries.filter(entry => ['user', 'steering', 'tool', 'summary', 'compaction_update'].includes(entry.kind)
    || ['assistant', 'thought'].includes(entry.kind) && visibleContent(entry.content));
  const last = relevant.at(-1);
  if (last?.kind !== 'assistant') return [];
  const result = [last];
  let messageId = last.message_id;
  for (let index = relevant.length - 2; index >= 0; index--) {
    const entry = relevant[index];
    if (entry.kind !== 'assistant' || messageId && entry.message_id && messageId !== entry.message_id) break;
    messageId ||= entry.message_id;
    result.unshift(entry);
  }
  return result;
}

export function turnPresentation(response: MessageGroup[], entries: ThreadEntry[], session: SessionSnapshot, latest: boolean, recorded?: SessionTurn) {
  const waiting = latest && session.permissions.length > 0;
  const running = latest && session.status === 'working' || waiting;
  let status = recorded?.status;
  if (running) status = 'in_progress';
  else if (status === 'in_progress') status = 'interrupted';
  else if (!status && latest) {
    if (session.error && !session.auth_required) status = 'failed';
    else if (session.stop_reason === 'end_turn') status = 'completed';
    else if (session.stop_reason === 'refusal') status = 'failed';
    else if (session.stop_reason) status = 'interrupted';
  }
  const candidate = !running && (!status || status === 'completed') ? finalMessageEntries(entries) : [];
  const dialogue = entries.some(entry => entry.kind === 'steering') ? entries.filter(isSteeringDialogue) : undefined;
  const finalIds = new Set(candidate.map(entry => entry.id));
  const select = (final: boolean) => response.flatMap(group => {
    const selected = group.entries.filter(entry => finalIds.has(entry.id) === final && (final || !dialogue || !isSteeringDialogue(entry)));
    return selected.length ? [{ ...group, id: selected[0].id, entries: selected }] : [];
  });
  return {
    running, waiting, status, process: select(false), conclusion: select(true),
    messages: dialogue ? messageGroups(dialogue, running, waiting) : undefined,
    reply: dialogueReply(dialogue ?? candidate),
    error: recorded?.error,
  };
}

export function groupProcess(groups: MessageGroup[]) {
  const items: { id: string; kind: 'tools' | 'message'; groups: MessageGroup[] }[] = [];
  for (const group of groups) {
    const last = items.at(-1);
    if (group.kind === 'tool' && last?.kind === 'tools') last.groups.push(group);
    else items.push({ id: group.id, kind: group.kind === 'tool' ? 'tools' : 'message', groups: [group] });
  }
  return items;
}
