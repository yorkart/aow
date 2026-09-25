export interface ArgumentsDraft {
  text: string;
  // Values already displayed as individual arguments must not be shell-parsed again.
  values: string[];
}

function argumentLine(value: string): string {
  if (value === '') return "''";
  if (/[\r\n]/.test(value)) return `$'${value.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/\r/g, '\\r').replace(/\n/g, '\\n')}'`;
  // Quote ambiguous literal values so editing their text cannot turn them into commands.
  return /^(?:["']|\$'|--?\S+[\t ])/.test(value) ? `'${value.replace(/'/g, "'\\''")}'` : value;
}

export function argumentsDraft(values: string[] = []): ArgumentsDraft {
  return { text: values.map(argumentLine).join('\n'), values };
}

function splitCommand(source: string): string[] {
  const values: string[] = [];
  let value = '';
  let started = false;
  let quote = '';
  const flush = () => {
    if (started) values.push(value);
    value = '';
    started = false;
  };
  for (let index = 0; index < source.length; index++) {
    const character = source[index];
    if (quote === "$'") {
      if (character === "'") quote = '';
      else if (character === '\\' && index + 1 < source.length) {
        const next = source[++index];
        const escapes: Record<string, string> = { n: '\n', r: '\r', t: '\t', b: '\b', f: '\f', v: '\v', '\\': '\\', "'": "'", '"': '"' };
        value += escapes[next] ?? `\\${next}`;
      } else value += character;
    } else if (quote === "'") {
      if (character === quote) quote = '';
      else value += character;
    } else if (character === '\\') {
      const rest = source.slice(index + 1);
      const continuation = rest.match(/^[\t ]*(?:\n|$)/);
      if (continuation) {
        index += continuation[0].length;
        continue;
      }
      const next = source[index + 1];
      if (quote === '"' && !['"', '\\', '$', '`'].includes(next)) value += character;
      else { value += next; index++; }
      started = true;
    } else if (quote) {
      if (character === quote) quote = '';
      else value += character;
    } else if (character === '$' && source[index + 1] === "'") {
      quote = "$'";
      started = true;
      index++;
    } else if (character === '"' || character === "'") {
      quote = character;
      started = true;
    } else if (/\s/.test(character)) {
      flush();
    } else {
      value += character;
      started = true;
    }
  }
  if (quote) throw new Error('Arguments 中的引号未闭合，请补全后再保存。');
  flush();
  return values;
}

export function normalizeArguments(draft: ArgumentsDraft, executable = ''): ArgumentsDraft {
  if (draft.text === argumentsDraft(draft.values).text) return draft;
  const knownLines = new Map(draft.values.map(value => [argumentLine(value), value]));
  const lines = draft.text.replace(/\r\n?/g, '\n').split('\n');
  const values: string[] = [];
  const commands = [executable.trim(), executable.trim().split('/').at(-1)].filter(Boolean);
  for (let index = 0; index < lines.length; index++) {
    const original = lines[index];
    if (knownLines.has(original)) {
      values.push(knownLines.get(original)!);
      continue;
    }
    let line = original.trimStart();
    if (!line.trim()) continue;
    // A bare value line can contain spaces, quotes, or backslashes literally.
    // Only command-shaped lines and explicitly quoted values need shell parsing.
    const startsExecutable = !values.length && commands.some(command => line.startsWith(`${command} `) || line.startsWith(`${command}\t`));
    const startsOption = /^--?\S+[\t ]/.test(line);
    const quoted = /^(?:["']|\$')/.test(line);
    if (!startsExecutable && !startsOption && !quoted) {
      values.push(original);
      continue;
    }
    // Join continuations and quoted multiline values before tokenizing.
    let parsed: string[];
    while (true) {
      const trailingSlashes = line.trimEnd().match(/\\+$/)?.[0].length ?? 0;
      if (trailingSlashes % 2 && index + 1 < lines.length) {
        line += `\n${lines[++index]}`;
        continue;
      }
      try {
        parsed = splitCommand(line);
        break;
      } catch (reason) {
        if (index + 1 >= lines.length) throw reason;
        line += `\n${lines[++index]}`;
      }
    }
    if (!values.length && parsed.length > 1 && commands.includes(parsed[0])) parsed.shift();
    values.push(...parsed);
  }
  return argumentsDraft(values);
}

export function pasteArguments(draft: ArgumentsDraft, text: string, start: number, end: number, executable = ''): { draft: ArgumentsDraft; caret: number } {
  const before = draft.text.slice(0, start);
  const after = draft.text.slice(end);
  const wholeLines = (!before || before.endsWith('\n')) && (!after || after.startsWith('\n'));
  const pasted = wholeLines ? normalizeArguments({ text, values: [] }, executable) : { text, values: [] };
  return {
    draft: { text: before + pasted.text + after, values: [...draft.values, ...pasted.values] },
    caret: before.length + pasted.text.length,
  };
}
