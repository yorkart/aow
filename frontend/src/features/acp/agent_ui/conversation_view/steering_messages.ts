import { text, type ThreadEntry } from '../../types';

// The saved transcript records arrival order. A submitted steer may arrive in
// the middle of a Markdown token; render that agent message continuously and
// keep the delivery card after it. Never mutate entries used by sync cursors.
export function coalesceSteeringChunks(entries: ThreadEntry[]): ThreadEntry[] {
  const result: ThreadEntry[] = [];
  let previousText: number | undefined;
  for (const entry of entries) {
    if (entry.kind === 'steering') { result.push(entry); continue; }
    const previous = previousText === undefined ? undefined : result[previousText];
    if (entry.kind === 'assistant' && entry.message_id && entry.content.type === 'text') {
      if (previous?.message_id === entry.message_id) {
        result[previousText!] = { ...previous, content: { ...previous.content, text: text(previous.content.text) + text(entry.content.text) } };
        continue;
      }
      previousText = result.length;
    } else previousText = undefined;
    result.push(entry);
  }
  return result;
}
