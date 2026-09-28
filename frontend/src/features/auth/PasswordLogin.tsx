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
      setError('请输入账号和密码。');
      return;
    }
    submittingRef.current = true;
    setSubmitting(true);
    setError(undefined);
    try {
      const result = await login('password', { username, password });
      onSuccess(result);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '登录失败，请重试。');
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

  return <>
    <p>请输入首次安装时设置的账号和密码。</p>
    <form onSubmit={onSubmit}>
      <label htmlFor="aow-username">账号</label>
      <input id="aow-username" name="username" autoComplete="username" autoFocus autoCapitalize="none" spellCheck={false} placeholder="请输入账号" type="text" value={username} required disabled={submitting} onChange={(event) => { setUsername(event.target.value); setError(undefined); }} />
      <label htmlFor="aow-password">密码</label>
      <input id="aow-password" name="password" autoComplete="current-password" placeholder="请输入密码" type="password" value={password} required disabled={submitting} onChange={(event) => { setPassword(event.target.value); setError(undefined); }} />
      {error && <p className="aow-auth-error" role="alert">{error}</p>}
      <button type="submit" disabled={submitting}>{submitting ? '正在验证…' : '登录'}</button>
    </form>
  </>;
}
