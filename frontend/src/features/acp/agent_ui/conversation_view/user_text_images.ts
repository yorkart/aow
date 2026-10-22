import { t } from '../../../../lib/i18n';
export interface UserTextPart { text: string; image?: string }

// Some agents replay pasted images as text: [@image](data:...) or ![alt](data:...).
// Keep the stored ACP content intact and interpret only these image wrappers.
export function userTextParts(value: string): UserTextPart[] {
  const parts: UserTextPart[] = [];
  const images = /(?:!\[[^\]\r\n]*\]|\[@image\])[ \t\r\n]*\([ \t\r\n]*data:(image\/(?:png|jpeg|webp|gif));base64,([A-Za-z0-9+/= \t\r\n]+)\)/gi;
  let offset = 0;
  for (const match of value.matchAll(images)) {
    const data = match[2].replace(/[ \t\r\n]/g, '');
    if (!/^[A-Za-z0-9+/]+={0,2}$/.test(data) || data.length % 4 === 1 || data.includes('=') && data.length % 4 !== 0) continue;
    if (match.index > offset) parts.push({ text: value.slice(offset, match.index) });
    parts.push({ text: match[0], image: `data:${match[1].toLowerCase()};base64,${data}` });
    offset = match.index + match[0].length;
  }
  if (offset < value.length) parts.push({ text: value.slice(offset) });
  return parts;
}

export function userTextPreview(value: string): string {
  return userTextParts(value).map(part => part.image ? t("acp.image") : part.text).join('');
}
