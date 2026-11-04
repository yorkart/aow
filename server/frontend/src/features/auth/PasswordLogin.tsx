import { t } from '../../lib/i18n';
import { FormEvent, useRef, useState } from 'react';
import { login } from './authApi';
import type { LoginMethodProps } from './types';

export function PasswordLogin({ onSuccess }: LoginMethodProps) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string>();
  const [submitting, setSubmitting] = useState(false);
  const submittingRef = useRef(false);

  const submit = async () => {
    if (submittingRef.current) return;
    if (!username || !password) {
      setError(t("auth.enter_your_username_and_password"));
      return;
    }
    submittingRef.current = true;
    setSubmitting(true);
    setError(undefined);
    try {
      const result = await login('password', { username, password });
      onSuccess(result);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : t("auth.sign_in_failed_try_again"));
      setPassword('');
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    void submit();
  };

  return <form onSubmit={onSubmit}>
    <label htmlFor="aow-username">{t("auth.username")}</label>
    <input id="aow-username" name="username" autoComplete="username" autoFocus autoCapitalize="none" spellCheck={false} placeholder={t("auth.enter_username")} type="text" value={username} required disabled={submitting} onChange={(event) => { setUsername(event.target.value); setError(undefined); }} />
    <label htmlFor="aow-password">{t("auth.password")}</label>
    <input id="aow-password" name="password" autoComplete="current-password" placeholder={t("auth.enter_password")} type="password" value={password} required disabled={submitting} onChange={(event) => { setPassword(event.target.value); setError(undefined); }} />
    {error && <p className="aow-auth-error" role="alert">{error}</p>}
    <button type="submit" disabled={submitting}>{submitting ? t("auth.verifying") : t("auth.sign_in")}</button>
  </form>;
}
