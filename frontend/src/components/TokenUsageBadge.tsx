import { ArrowDownToLine, ArrowUpFromLine } from 'lucide-react';
import { formatTokenCount, tokenUsageDetails, tokenUsageLabel, type TokenUsage } from '../lib/tokenUsage';
import './token-usage.css';

export function TokenUsageBadge({ usage, split = false }: { usage?: TokenUsage; split?: boolean }) {
  if (!usage) return null;
  if (split) return <small className="token-usage token-usage-split" title={tokenUsageDetails(usage)}>
    <span className="token-usage-input" aria-label={`输入 ${formatTokenCount(usage.input_tokens)}`}><ArrowDownToLine size={12} aria-hidden="true" />{formatTokenCount(usage.input_tokens)}</span>
    <span className="token-usage-output" aria-label={`输出 ${formatTokenCount(usage.output_tokens)}`}><ArrowUpFromLine size={12} aria-hidden="true" />{formatTokenCount(usage.output_tokens)}</span>
  </small>;
  return <small className="token-usage" title={tokenUsageDetails(usage)}>{tokenUsageLabel(usage)}</small>;
}
