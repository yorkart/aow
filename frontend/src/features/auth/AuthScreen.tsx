import { t } from '../../lib/i18n';
import type { ReactNode } from 'react';

export function AuthScreen({ title, detail, children, retry }: {
  title: string;
  detail?: string;
  children?: ReactNode;
  retry?: () => void;
}) {
  return <main className="aow-auth">
    <section className={`aow-auth-card${children ? '' : ' aow-auth-message'}`} aria-labelledby="aow-auth-title" aria-live={children ? undefined : 'polite'}>
      <div className="aow-auth-heading">
        <div className="aow-auth-mark" aria-hidden="true">A<span /></div>
        <h1 id="aow-auth-title">{title}</h1>
      </div>
      {detail && <p>{detail}</p>}
      {children}
      {retry && <button type="button" onClick={retry}>{t("aow.retry")}</button>}
    </section>
  </main>;
}
