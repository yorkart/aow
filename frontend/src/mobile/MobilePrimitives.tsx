import { t, getLanguage } from '../lib/i18n';
import { ArrowLeft, LoaderCircle, RefreshCw } from 'lucide-react';
import type { ReactNode } from 'react';

export function MobileState({ loading, error, empty, retry }: { loading?: boolean; error?: string; empty?: string; retry?: () => void }) {
  if (error) return <div className="mobile-state error" role="alert"><p>{error}</p><button className="mobile-button" onClick={retry}><RefreshCw size={16} />{t("configuration.reload")}</button></div>;
  if (loading) return <div className="mobile-state" role="status"><LoaderCircle className="spinning" size={22} /><p>{t("aow.loading")}</p></div>;
  return empty ? <div className="mobile-state"><p>{empty}</p></div> : null;
}

export function MobilePageHeader({ title, subtitle, back, backLabel = t("mobile.back"), actions }: { title: string; subtitle?: string; back?: () => void; backLabel?: string; actions?: ReactNode }) {
  return <header className="mobile-page-header">
    {back && <button type="button" className="mobile-icon-button" aria-label={backLabel} onClick={back}><ArrowLeft size={21} /></button>}
    <div className="mobile-heading"><h2 title={title}>{title}</h2>{subtitle && <span>{subtitle}</span>}</div>
    {actions}
  </header>;
}

export function MobileRefresh({ reload, loading }: { reload: () => void; loading?: boolean }) {
  return <button className="mobile-icon-button" aria-label={t("files.refresh")} disabled={loading} onClick={reload}><RefreshCw size={18} className={loading ? 'spinning' : ''} /></button>;
}

export function MobilePanelTabs<T extends string>({ label, items, selected, onSelect }: {
  label: string; items: readonly { id: T; label: string }[]; selected: T; onSelect: (id: T) => void;
}) {
  return <div className="mobile-panel-tabs" role="tablist" aria-label={label}>
    {items.map((item, index) => <button key={item.id} type="button" role="tab" aria-selected={selected === item.id}
      tabIndex={selected === item.id ? 0 : -1} onClick={() => onSelect(item.id)} onKeyDown={event => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1
          : (index + (event.key === 'ArrowRight' ? 1 : -1) + items.length) % items.length;
        event.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>('[role=tab]')[next]?.focus();
        onSelect(items[next].id);
      }}>{item.label}</button>)}
  </div>;
}

export function mobileTime(value: string) {
  return new Date(value).toLocaleString(getLanguage(), { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
}
