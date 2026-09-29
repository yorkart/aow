import type { ReactNode } from 'react';
import './aow-list-row.css';

export function AowListRowStatus({ tone = 'muted', children }: {
  tone?: 'muted' | 'success' | 'error';
  children: ReactNode;
}) {
  return <span className="aow-list-row-status" data-tone={tone}><i aria-hidden="true" />{children}</span>;
}

export function AowListRowMeta({ status, children }: { status: ReactNode; children: ReactNode }) {
  return <small className="aow-list-row-meta">
    {status}<span aria-hidden="true">•</span><span className="aow-list-row-summary">{children}</span>
  </small>;
}
