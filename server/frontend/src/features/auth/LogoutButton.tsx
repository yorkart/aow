import { t } from '../../lib/i18n';
import { useState } from 'react';
import { LogOut } from 'lucide-react';
import { logout } from './authApi';

export function LogoutButton({ disabled = false, compact = false, className }: { disabled?: boolean; compact?: boolean; className?: string }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const leave = async () => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await logout();
      window.location.reload();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("auth.sign_out_failed_try_again"));
      setBusy(false);
    }
  };
  return <>
    <button type="button" className={className} disabled={disabled || busy} aria-label={t("auth.sign_out")} title={t("auth.sign_out")} onClick={() => void leave()}>
      <LogOut size={18} />{!compact && <span>{busy ? t("auth.signing_out") : t("auth.sign_out")}</span>}
    </button>
    {error && <span role="alert" className="aow-auth-error">{error}</span>}
  </>;
}
