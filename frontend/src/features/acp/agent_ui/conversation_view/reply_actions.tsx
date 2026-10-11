import { useState } from 'react';
import { copyText } from './clipboard';
import { ReplyActionIcon } from './reply_action_icons';

export function ReplyActions({ content, onJump, onScrollToTop }: { content: string; onJump: () => void; onScrollToTop?: () => void }) {
  const [error, setError] = useState(false);
  return <div className="acp-message-actions">
    {!!content && <button type="button" aria-label="复制回复" title="Copy This Agent Response" onClick={() => {
      setError(false);
      void copyText(content).catch(() => setError(true));
    }}><ReplyActionIcon name="copy" /></button>}
    <button type="button" aria-label="跳到用户消息" title="Scroll to User Message" onClick={onJump}><ReplyActionIcon name="user_arrow_up" /></button>
    {onScrollToTop && <button type="button" aria-label="回到顶部" title="Scroll to Top" onClick={onScrollToTop}><ReplyActionIcon name="arrow_up" /></button>}
    {error && <span className="acp-error" role="alert">复制失败，请选择文本后手动复制。</span>}
  </div>;
}
