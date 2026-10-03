export interface EnvironmentDraft {
  text: string;
  values: Record<string, string>;
}

function environmentLine(key: string, value: string): string {
  // Keep existing multiline values on one display line without losing them on save.
  const display = /[\r\n]/.test(value)
    ? value.replace(/\\/g, '\\\\').replace(/\r/g, '\\r').replace(/\n/g, '\\n')
    : value;
  return `${key}=${display}`;
}

export function environmentDraft(values: Record<string, string> = {}): EnvironmentDraft {
  return { text: Object.entries(values).map(([key, value]) => environmentLine(key, value)).join('\n'), values };
}

export function parseEnvironment(draft: EnvironmentDraft): Record<string, string> {
  const values = new Map<string, string>();
  const knownLines = new Map(Object.entries(draft.values).map(([key, value]) => [environmentLine(key, value), value]));
  const lines = draft.text.split(/\r\n?|\n/);
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    if (!line.trim()) continue;
    const separator = line.indexOf('=');
    const key = line.slice(0, separator).trim();
    if (separator < 1 || !/^[A-Za-z0-9_]+$/.test(key)) {
      throw new Error(`Environment variables 第 ${index + 1} 行格式无效，请使用 key=value，变量名只能包含字母、数字和下划线。`);
    }
    if (values.has(key)) throw new Error(`Environment variables 第 ${index + 1} 行的变量名重复，请每个变量只填写一次。`);
    const value = knownLines.get(line) ?? line.slice(separator + 1);
    if (value.includes('\0')) throw new Error(`Environment variables 第 ${index + 1} 行的值不能包含空字符。`);
    values.set(key, value);
  }
  return Object.fromEntries(values);
}
