import type { PromptBinding } from './types';

/** Only the editor discovers variables; all other views use saved bindings. */
export function identifyVariables(prompt: string): PromptBinding[] {
  const encoder = new TextEncoder();
  const bindings: PromptBinding[] = [];
  const pattern = /\{\{\s*([^{}\s]+)\s*\}\}/gu;
  let previousIndex = 0;
  let byteOffset = 0;
  for (const match of prompt.matchAll(pattern)) {
    byteOffset += encoder.encode(prompt.slice(previousIndex, match.index)).length;
    const end = byteOffset + encoder.encode(match[0]).length;
    bindings.push({ name: match[1], placeholder: match[0], start: byteOffset, end });
    previousIndex = match.index + match[0].length;
    byteOffset = end;
  }
  return bindings;
}

export function variableNames(bindings: PromptBinding[] = []): string[] {
  return [...new Set(bindings.map(binding => binding.name))];
}
