import { t } from '../../../../lib/i18n';
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
    {!!content && <button type="button" aria-label={t("acp.copy_response")} title={copySucceeded ? t("components.copied") : t("common.copy_this_agent_response")} onClick={() => {
      setCopied(undefined);
      setError(false);
      void copyText(content).then(() => setCopied({ content })).catch(() => setError(true));
    }}>{copySucceeded ? <Check size={14} aria-hidden="true" /> : <ReplyActionIcon name="copy" />}</button>}
    <button type="button" aria-label={t("acp.jump_to_user_message")} title={t("navigation.scroll_to_user_message")} onClick={onJump}><ReplyActionIcon name="user_arrow_up" /></button>
    {onScrollToTop && <button type="button" aria-label={t("acp.back_to_top")} title={t("navigation.scroll_to_top")} onClick={onScrollToTop}><ReplyActionIcon name="arrow_up" /></button>}
    {error && <span className="acp-error" role="alert">{t("components.copy_failed_select_the_text_and_copy_it_manually")}</span>}
  </div>;
}
