import { marked, type Token } from 'marked';

export type RoleReference = { type: 'role'; id: string; name: string };
export type MentionPart = { type: 'text'; text: string } | RoleReference;
export const roleMentionText = (role: Pick<RoleReference, 'id' | 'name'>) => `[@${role.name.replace(/[\\[\]]/g, '\\$&').replace(/[\r\n]/g, ' ')}](mention://role/${role.id})`;
export const roleInitial = (name: string) => Array.from(name.trim())[0]?.toLocaleUpperCase() || '@';

// Parse Markdown links, retaining original text and ignoring code, images and HTML.
// This is display/clipboard decoding only. Dispatch always goes through the model
// and the server's validated collaboration API.
export function mentionParts(value: string): MentionPart[] {
  const mentions: { start: number; end: number; role: RoleReference }[] = [];
  const visit = (tokens: Token[], source: string, base: number) => {
    let cursor = 0;
    for (const token of tokens) {
      const start = source.indexOf(token.raw, cursor);
      if (start < 0) continue;
      cursor = start + token.raw.length;
      if (token.type === 'link' && token.raw.startsWith('[@')) {
        const match = /^mention:\/\/role\/([a-zA-Z0-9_-]{1,128})$/.exec(token.href);
        if (match && token.raw.endsWith(`](mention://role/${match[1]})`)) mentions.push({ start: base + start, end: base + cursor, role: { type: 'role', id: match[1], name: token.text.slice(1).replace(/\\([\\[\]])/g, '$1') } });
      } else if (['paragraph', 'text', 'heading'].includes(token.type) && 'tokens' in token && token.tokens) {
        visit(token.tokens, token.raw, base + start);
      }
    }
  };
  visit(marked.lexer(value), value, 0);
  const parts: MentionPart[] = [];
  let cursor = 0;
  for (const mention of mentions) {
    if (mention.start > cursor) parts.push({ type: 'text', text: value.slice(cursor, mention.start) });
    parts.push(mention.role); cursor = mention.end;
  }
  if (cursor < value.length) parts.push({ type: 'text', text: value.slice(cursor) });
  return parts;
}

export const mentionPreview = (value: string) => mentionParts(value).map(part => part.type === 'role' ? `@${part.name}` : part.text).join('');
