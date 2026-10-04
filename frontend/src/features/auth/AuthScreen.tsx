import type { ReactNode } from 'react';

export function AuthScreen({ title, detail, children, retry }: {
  title: string;
  detail?: string;
  children?: ReactNode;
  retry?: () => void;
}) {
  return <main className="aow-auth">
    <section className={`aow-auth-card${children ? '' : ' aow-auth-message'}`} aria-labelledby="aow-auth-title" aria-live={children ? undefined : 'polite'}>
      <div className="aow-auth-mark" aria-hidden="true">A<span /></div>
      <p className="aow-auth-eyebrow">AoW</p>
      <h1 id="aow-auth-title">{title}</h1>
      {detail && <p>{detail}</p>}
      {children}
      {retry && <button type="button" onClick={retry}>重试</button>}
    </section>
  </main>;
}
