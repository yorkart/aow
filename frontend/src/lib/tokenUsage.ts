export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cached_input_tokens: number;
  cache_write_input_tokens: number;
  reasoning_output_tokens: number;
  total_tokens: number;
}

export function parseTokenUsage(value: unknown): TokenUsage | undefined {
  if (!value || typeof value !== 'object') return;
  const data = value as Record<string, unknown>;
  const count = (key: keyof TokenUsage, optional = false) => {
    const value = data[key] ?? (optional ? 0 : undefined);
    return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : undefined;
  };
  const input_tokens = count('input_tokens');
  const output_tokens = count('output_tokens');
  const total_tokens = count('total_tokens');
  const cached_input_tokens = count('cached_input_tokens', true);
  const cache_write_input_tokens = count('cache_write_input_tokens', true);
  const reasoning_output_tokens = count('reasoning_output_tokens', true);
  if (input_tokens === undefined || output_tokens === undefined || total_tokens === undefined
    || cached_input_tokens === undefined || cache_write_input_tokens === undefined || reasoning_output_tokens === undefined) return;
  return { input_tokens, output_tokens, total_tokens, cached_input_tokens, cache_write_input_tokens, reasoning_output_tokens };
}

const compact = new Intl.NumberFormat('en-US', { notation: 'compact', maximumFractionDigits: 1 });
const exact = new Intl.NumberFormat('en-US');

export const formatTokenCount = (count: number) => `${compact.format(count)} tokens`;
export const tokenUsageLabel = (usage: TokenUsage) => `本轮 ${formatTokenCount(usage.total_tokens)}`;

export function tokenUsageDetails(usage: TokenUsage): string {
  const details = [
    `本轮：${exact.format(usage.total_tokens)} tokens`,
    `输入：${exact.format(usage.input_tokens)}`,
    `输出：${exact.format(usage.output_tokens)}`,
    `缓存命中（包含在输入中）：${exact.format(usage.cached_input_tokens)}`,
  ];
  if (usage.cache_write_input_tokens) details.push(`缓存写入（包含在输入中）：${exact.format(usage.cache_write_input_tokens)}`);
  if (usage.reasoning_output_tokens) details.push(`推理（包含在输出中）：${exact.format(usage.reasoning_output_tokens)}`);
  return details.join('\n');
}
