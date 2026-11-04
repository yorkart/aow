import { t } from '../../../../lib/i18n';
import type { Data } from '../../types';
import { Notice } from './activity';
import { sessionMetaFormats } from './session_meta_formats';

export function SessionMeta({ metadata }: { metadata?: Data }) {
  if (!metadata) return null;
  return <section className="acp-session-meta" aria-label={t("acp.session_metadata")}>
    {sessionMetaFormats.map(({ key, format }) => {
      const message = format(metadata[key]);
      return message ? <Notice key={key} notice={{ id: key, kind: 'notice', content: { title: message, severity: 'info' } }} /> : null;
    })}
  </section>;
}
