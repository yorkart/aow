import { t } from '../../../../lib/i18n';
import { useMemo, useState } from 'react';
import { userTextParts } from './user_text_images';
import { RoleMentionText } from '../../../agent-roles/RoleMention';

export function UserText({ text }: { text: string }) {
  const parts = useMemo(() => userTextParts(text), [text]);
  return <div className="acp-user-text conversation-prose">{parts.map((part, index) => part.image
    ? <ReplayedImage key={index} source={part.image} original={part.text} /> : <RoleMentionText key={index} text={part.text} />)}</div>;
}

function ReplayedImage({ source, original }: { source: string; original: string }) {
  const [failedSource, setFailedSource] = useState('');
  // Syntactically valid base64 may still contain truncated or non-image bytes.
  if (failedSource === source) return <>{original}</>;
  return <img className="acp-image" alt={t("navigation.user_image")} src={source} onError={() => setFailedSource(source)} />;
}
