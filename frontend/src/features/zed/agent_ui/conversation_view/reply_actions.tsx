import { useState } from 'react';
import { ArrowUp, Check, Copy } from 'lucide-react';

async function copyReply(value: string) {
  try {
    if (navigator.clipboard) { await navigator.clipboard.writeText(value); return; }
  } catch { /* Insecure origins and denied clipboard permissions use the fallback. */ }
  const focused = document.activeElement;
  const selection = window.getSelection();
  const ranges = Array.from({ length: selection?.rangeCount ?? 0 }, (_, index) => selection!.getRangeAt(index).cloneRange());
  const input = document.createElement('textarea');
  input.value = value; input.readOnly = true; input.className = 'zed-clipboard-input';
  document.body.append(input);
  try {
    input.select();
    if (!document.execCommand('copy')) throw new Error('copy failed');
  } finally {
    input.remove();
    if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
    selection?.removeAllRanges(); ranges.forEach(range => selection?.addRange(range));
  }
}

export function ReplyActions({ content, onJump }: { content: string; onJump: () => void }) {
  const [copied, setCopied] = useState('');
  const [error, setError] = useState(false);
  return <div className="zed-message-actions">
    <button type="button" aria-label="复制回复" title="复制回复" onClick={() => {
      setError(false);
      void copyReply(content).then(() => setCopied(content)).catch(() => { setCopied(''); setError(true); });
    }}>{copied === content ? <Check size={14} /> : <Copy size={14} />}</button>
    <button type="button" aria-label="跳到用户消息" title="跳到用户消息" onClick={onJump}><ArrowUp size={14} /></button>
    {error && <span className="zed-error" role="alert">复制失败，请选择文本后手动复制。</span>}
  </div>;
}
