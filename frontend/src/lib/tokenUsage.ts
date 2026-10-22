import { t, getLanguage } from './i18n';
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

const compact = new Intl.NumberFormat(getLanguage(), { notation: 'compact', maximumFractionDigits: 1 });
const exact = new Intl.NumberFormat(getLanguage());

export const formatTokenCount = (count: number) => `${compact.format(count)} tokens`;
export const tokenUsageLabel = (usage: TokenUsage) => t("lib.this_turn", { value0: formatTokenCount(usage.total_tokens) });

export function tokenUsageDetails(usage: TokenUsage): string {
  const details = [
    t("lib.this_turn_tokens", { value0: exact.format(usage.total_tokens) }),
    t("lib.input", { value0: exact.format(usage.input_tokens) }),
    t("lib.output", { value0: exact.format(usage.output_tokens) }),
    t("lib.cache_hits_included_in_input", { value0: exact.format(usage.cached_input_tokens) }),
  ];
  if (usage.cache_write_input_tokens) details.push(t("lib.cache_writes_included_in_input", { value0: exact.format(usage.cache_write_input_tokens) }));
  if (usage.reasoning_output_tokens) details.push(t("lib.reasoning_included_in_output", { value0: exact.format(usage.reasoning_output_tokens) }));
  return details.join('\n');
}
