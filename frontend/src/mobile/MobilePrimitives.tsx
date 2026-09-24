import { ArrowLeft, LoaderCircle, RefreshCw } from 'lucide-react';
import type { ReactNode } from 'react';

export function MobileState({ loading, error, empty, retry }: { loading?: boolean; error?: string; empty?: string; retry?: () => void }) {
  if (error) return <div className="mobile-state error" role="alert"><p>{error}</p><button className="mobile-button" onClick={retry}><RefreshCw size={16} />重新加载</button></div>;
  if (loading) return <div className="mobile-state" role="status"><LoaderCircle className="spinning" size={22} /><p>正在加载…</p></div>;
  return empty ? <div className="mobile-state"><p>{empty}</p></div> : null;
}

export function MobilePageHeader({ title, subtitle, back, actions }: { title: string; subtitle?: string; back?: () => void; actions?: ReactNode }) {
  return <header className="mobile-page-header">
    {back && <button className="mobile-icon-button" aria-label="返回" onClick={back}><ArrowLeft size={21} /></button>}
    <div className="mobile-heading"><h2>{title}</h2>{subtitle && <span>{subtitle}</span>}</div>
    {actions}
  </header>;
}

export function MobileRefresh({ reload, loading }: { reload: () => void; loading?: boolean }) {
  return <button className="mobile-icon-button" aria-label="刷新" disabled={loading} onClick={reload}><RefreshCw size={18} className={loading ? 'spinning' : ''} /></button>;
}

export function mobileTime(value: string) {
  return new Date(value).toLocaleString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
}
