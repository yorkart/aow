import { t } from '../../../../lib/i18n';
import { contentMarkdown, messageGroups, visibleContent, type MessageGroup } from './entry_view_state';
import { object, text, type Data, type ProcessSegment, type SessionSnapshot, type SessionTurn, type ThreadEntry, type TurnOverview } from '../../types';
import { userTextPreview } from './user_text_images';
import { coalesceSteeringChunks } from './steering_messages';

function dialogueReply(entries: ThreadEntry[]) {
  return coalesceSteeringChunks(entries).filter(entry => entry.kind === 'assistant')
    .map(entry => contentMarkdown(entry.content)).filter(Boolean).join('\n\n');
}

export function overviewPresentation(turn: TurnOverview): ReturnType<typeof turnPresentation> {
  const steering = new Map(messageGroups((turn.messages ?? []).filter(entry => entry.kind === 'steering'), false, false).map(group => [group.id, group]));
  return {
    running: turn.running, waiting: turn.waiting, status: turn.status ?? undefined,
    process: [], conclusion: messageGroups(turn.conclusion, false, false),
    segments: (turn.process_segments ?? [{ ...turn, id: turn.id }]).map(segment => ({ ...segment, process: [], steering: steering.get(segment.id) })),
    reply: dialogueReply(turn.conclusion),
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
  const isRelevant = (entry: ThreadEntry) => ['user', 'steering', 'tool', 'summary', 'compaction_update'].includes(entry.kind)
    || ['assistant', 'thought'].includes(entry.kind) && visibleContent(entry.content);
  const visible = entries.filter(isRelevant);
  const last = visible.at(-1);
  if (last?.kind !== 'assistant') return [];
  // Whitespace fragments can carry Markdown indentation and the coalesced
  // message's render ID. Keep them when the final message identity is known.
  const relevant = last.message_id ? entries.filter(entry => isRelevant(entry)
    || entry.kind === 'assistant' && entry.message_id === last.message_id && entry.content.type === 'text') : visible;
  const result = [relevant.at(-1)!];
  let messageId = last.message_id;
  for (let index = relevant.length - 2; index >= 0; index--) {
    const entry = relevant[index];
    if (entry.kind === 'steering' && messageId) {
      let before = index - 1;
      while (before >= 0 && relevant[before].kind === 'steering') before--;
      if (before >= 0 && relevant[before].kind === 'assistant' && relevant[before].message_id === messageId) continue;
    }
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
  const finalIds = new Set(candidate.map(entry => entry.id));
  const select = (final: boolean) => response.flatMap(group => {
    const selected = group.entries.filter(entry => finalIds.has(entry.id) === final);
    return selected.length ? [{ ...group, id: selected[0].id, entries: selected }] : [];
  });
  const process = select(false);
  const permissionTools = new Set(session.permissions.map(permission => text(object(permission.request.toolCall).toolCallId)));
  return {
    running, waiting, status, process, conclusion: select(true),
    segments: processSegments(process, entries[0]?.kind === 'user' ? entries[0].id : 'leading', permissionTools),
    reply: dialogueReply(candidate),
    error: recorded?.error,
  };
}

export interface PresentedSegment extends ProcessSegment { process: MessageGroup[]; steering?: MessageGroup }

// Steering is a visual boundary within a turn, never a new task or a final answer.
export function processSegments(groups: MessageGroup[], id: string, permissionTools = new Set<string>()): PresentedSegment[] {
  const segment = (id: string, steering?: MessageGroup): PresentedSegment => ({ id, steering, process: [], tool_count: 0, progress_count: 0, permission_tool_ids: [] });
  const segments = [segment(id)];
  for (const group of groups) {
    if (group.kind === 'user') continue;
    if (group.kind === 'steering') { segments.push(segment(group.id, group)); continue; }
    const current = segments.at(-1)!;
    current.process.push(group);
    if (group.kind === 'tool') {
      current.tool_count++;
      if (permissionTools.has(group.id)) current.permission_tool_ids.push(group.id);
    } else current.progress_count++;
  }
  return segments;
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
