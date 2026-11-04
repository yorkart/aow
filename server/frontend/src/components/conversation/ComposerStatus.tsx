import { t } from '../../lib/i18n';
import type { ReactNode } from 'react';
import './composer-status.css';

export function ComposerStatus({ icon, title, description, actions, children, label = t("components.session_status"), alert = false }: {
  icon: ReactNode; title: string; description?: ReactNode; actions?: ReactNode; children?: ReactNode; label?: string; alert?: boolean;
}) {
  return <section className="conversation-composer-status" aria-label={label}>
    <div className="conversation-composer-status-header">
      <span className="conversation-composer-status-icon" aria-hidden="true">{icon}</span>
      <div className="conversation-composer-status-message" role={alert ? 'alert' : 'status'}>
        <strong>{title}</strong>
        {description && <div className="conversation-composer-status-description">{description}</div>}
      </div>
      {actions && <div className="conversation-composer-status-actions">{actions}</div>}
    </div>
    {children && <div className="conversation-composer-status-details">{children}</div>}
  </section>;
}
