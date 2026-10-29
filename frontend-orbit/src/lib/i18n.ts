import chinese from './zh-CN.json';
import english from './en-US.json';

// Orbit owns its small login dictionary; no imports from the original frontend.
const messages = navigator.language.startsWith('zh') ? chinese : english;
export function t(key: string, values: Record<string, unknown> = {}): string {
  const [group, name] = key.split('.');
  const section = messages[group as keyof typeof messages] as Record<string, string> | undefined;
  return (section?.[name] ?? key).replace(/\{\{(\w+)\}\}/g, (_, token: string) => String(values[token] ?? ''));
}
