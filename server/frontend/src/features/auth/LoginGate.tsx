import { t } from '../../lib/i18n';
import { ReactNode, useEffect, useState } from 'react';
import { authStatus } from './authApi';
import { AuthScreen } from './AuthScreen';
import { loginMethods } from './loginMethods';
import type { AuthStatus, LoginMethodProps } from './types';
import './auth.css';

export function LoginGate({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>();
  const [loadError, setLoadError] = useState<string>();

  useEffect(() => {
    let active = true;
    void authStatus()
      .then((next) => { if (active) setStatus(next); })
      .catch((error: unknown) => { if (active) setLoadError(error instanceof Error ? error.message : t("auth.unable_to_read_login_status")); });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    if (!status?.authenticated) return;
    let active = true;
    const check = () => {
      if (document.visibilityState === 'hidden') return;
      void authStatus().then(next => { if (active) setStatus(next); }).catch(() => {});
    };
    const timer = window.setInterval(check, 30_000);
    document.addEventListener('visibilitychange', check);
    return () => { active = false; window.clearInterval(timer); document.removeEventListener('visibilitychange', check); };
  }, [status?.authenticated]);

  if (loadError) return <AuthScreen title={t("auth.unable_to_connect_to_aow")} detail={loadError} retry={() => window.location.reload()} />;
  if (!status) return <AuthScreen title={t("auth.checking_login_status")} />;
  if (!status.configured) return <AuthScreen title={t("auth.no_login_method_configured")} detail={status.message ?? t("auth.complete_login_configuration_on_the_server")} />;
  if (status.authenticated) return <>{children}</>;
  return <LoginMethods status={status} onSuccess={setStatus} />;
}

function LoginMethods({ status, onSuccess }: { status: AuthStatus } & LoginMethodProps) {
  const [selected, setSelected] = useState<string>();
  const available = (status.methods ?? [{ id: 'password', label: t("auth.username_and_password"), configured: true }])
    .filter(method => method.configured && loginMethods.has(method.id));
  const method = available.find(method => method.id === selected) ?? available[0];
  const Form = method && loginMethods.get(method.id);
  if (!Form) return <AuthScreen title={t("auth.this_login_method_is_unsupported_by_this_page")} detail={t("auth.refresh_the_page_or_update_the_client_and_try")} retry={() => window.location.reload()} />;

  return <AuthScreen title={t("auth.sign_in_to_aow")}>
    {available.length > 1 && <div className="aow-auth-methods" role="group" aria-label={t("auth.login_method")}>
      {available.map(option => <button key={option.id} type="button" aria-pressed={method.id === option.id} onClick={() => setSelected(option.id)}>{option.label}</button>)}
    </div>}
    <Form key={method.id} onSuccess={onSuccess} />
  </AuthScreen>;
}
