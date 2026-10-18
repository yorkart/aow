import { useState } from 'react';
import { Check, Copy } from 'lucide-react';
import { copyText } from '../../../../lib/clipboard';

export function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState<string>();
  const [error, setError] = useState(false);
  return <>
    <button type="button" className="acp-quiet-button acp-copy-button" aria-label={label} title={label} onClick={() => {
      setError(false);
      void copyText(text).then(() => setCopied(text)).catch(() => setError(true));
    }}>{copied === text ? <Check size={13} /> : <Copy size={13} />}</button>
    {error && <span className="acp-copy-error" role="alert">复制失败，请选择文本后手动复制。</span>}
  </>;
}
