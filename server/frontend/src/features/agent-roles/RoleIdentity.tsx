import { useEffect, useState } from 'react';
import { t } from '../../lib/i18n';
import type { IdentitySource } from './api';

export function RoleIdentity({ preset, source, onSourceChange, disabled }: {
  preset?: IdentitySource; source: string; onSourceChange: (source: string) => void; disabled: boolean;
}) {
  const [sourceEdited, setSourceEdited] = useState(false);
  useEffect(() => {
    if (!sourceEdited) onSourceChange(preset?.source ?? '');
  }, [preset, sourceEdited, onSourceChange]);
  return <>
    <label className="project-aow-dialog-field"><span>{t('roles.source_home')}</span><input value={source} disabled={disabled} spellCheck={false} placeholder="~/" onChange={event => {
      setSourceEdited(true); onSourceChange(event.target.value);
    }} /></label>
    <p className="project-aow-form-intro">{t('roles.known_identity_hint')}</p>
  </>;
}
