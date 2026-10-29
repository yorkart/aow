import { parseDocument } from 'yaml';
import { HttpError } from '../errors.js';

export function skillName(value: unknown): string {
  if (typeof value !== 'string' || !value.trim() || [...value.trim()].length > 80 || /[/\\\p{Cc}]/u.test(value) || ['.', '..'].includes(value.trim())) {
    throw new HttpError(422, 'invalid_skill_name', '技能名称须为 1–80 个字符，不能包含路径分隔符或控制字符。');
  }
  return value.trim();
}

export function description(value: unknown): string {
  if (typeof value !== 'string' || !value.trim() || [...value.trim()].length > 2000) throw new HttpError(422, 'invalid_description', '请填写技能说明（最多 2000 个字符）。');
  return value.trim();
}

function frontmatter(content: string) {
  const match = /^(?:\uFEFF)?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(content);
  if (!match) throw new HttpError(422, 'invalid_skill_entry', 'SKILL.md 必须包含 YAML frontmatter，以及 name 和 description。');
  const document = parseDocument(match[1], { uniqueKeys: true });
  if (document.errors.length) throw new HttpError(422, 'invalid_skill_entry', 'SKILL.md 的 YAML frontmatter 格式无效。');
  let value: unknown;
  try { value = document.toJS({ maxAliasCount: 0 }); }
  catch { throw new HttpError(422, 'invalid_skill_entry', 'SKILL.md 不支持 YAML 别名。'); }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new HttpError(422, 'invalid_skill_entry', 'SKILL.md 的 frontmatter 必须为对象。');
  return { match, document, value: value as Record<string, unknown> };
}

export function metadata(content: string) {
  const { value } = frontmatter(content);
  return { name: skillName(value.name), description: description(value.description) };
}

export function createEntry(name: unknown, detail: unknown, instructions: unknown): string {
  if (typeof instructions !== 'string' || !instructions.trim()) throw new HttpError(422, 'invalid_instructions', '请填写技能指令。');
  return `---\nname: ${JSON.stringify(skillName(name))}\ndescription: ${JSON.stringify(description(detail))}\n---\n\n${instructions}\n`;
}

export function renameEntry(content: string, name: unknown): string {
  const { match, document } = frontmatter(content);
  document.set('name', skillName(name));
  return `---\n${document.toString()}---\n${content.slice(match[0].length)}`;
}
