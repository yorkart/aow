import { useEffect, useState } from 'react';
import { Check } from 'lucide-react';
import { copyText } from '../../../../lib/clipboard';
import { ReplyActionIcon } from './reply_action_icons';

export function ReplyActions({ content, onJump, onScrollToTop }: { content: string; onJump: () => void; onScrollToTop?: () => void }) {
  const [copied, setCopied] = useState<{ content: string }>();
  const [error, setError] = useState(false);
  const copySucceeded = copied?.content === content;
  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(undefined), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return <div className="acp-message-actions">
    {!!content && <button type="button" aria-label="复制回复" title={copySucceeded ? '已复制' : 'Copy This Agent Response'} onClick={() => {
      setCopied(undefined);
      setError(false);
      void copyText(content).then(() => setCopied({ content })).catch(() => setError(true));
    }}>{copySucceeded ? <Check size={14} aria-hidden="true" /> : <ReplyActionIcon name="copy" />}</button>}
    <button type="button" aria-label="跳到用户消息" title="Scroll to User Message" onClick={onJump}><ReplyActionIcon name="user_arrow_up" /></button>
    {onScrollToTop && <button type="button" aria-label="回到顶部" title="Scroll to Top" onClick={onScrollToTop}><ReplyActionIcon name="arrow_up" /></button>}
    {error && <span className="acp-error" role="alert">复制失败，请选择文本后手动复制。</span>}
  </div>;
}
