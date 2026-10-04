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
      .catch((error: unknown) => { if (active) setLoadError(error instanceof Error ? error.message : '无法读取登录状态'); });
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

  if (loadError) return <AuthScreen title="无法连接 AoW" detail={loadError} retry={() => window.location.reload()} />;
  if (!status) return <AuthScreen title="正在检查登录状态…" />;
  if (!status.configured) return <AuthScreen title="尚未配置登录方式" detail={status.message ?? '请在服务器上完成登录配置。'} />;
  if (status.authenticated) return <>{children}</>;
  return <LoginMethods status={status} onSuccess={setStatus} />;
}

function LoginMethods({ status, onSuccess }: { status: AuthStatus } & LoginMethodProps) {
  const [selected, setSelected] = useState<string>();
  const available = (status.methods ?? [{ id: 'password', label: '账号密码', configured: true }])
    .filter(method => method.configured && loginMethods.has(method.id));
  const method = available.find(method => method.id === selected) ?? available[0];
  const Form = method && loginMethods.get(method.id);
  if (!Form) return <AuthScreen title="当前页面不支持此登录方式" detail="请刷新页面或更新客户端后重试。" retry={() => window.location.reload()} />;

  return <AuthScreen title="登录 AoW">
    {available.length > 1 && <div className="aow-auth-methods" role="group" aria-label="登录方式">
      {available.map(option => <button key={option.id} type="button" aria-pressed={method.id === option.id} onClick={() => setSelected(option.id)}>{option.label}</button>)}
    </div>}
    <Form key={method.id} onSuccess={onSuccess} />
  </AuthScreen>;
}
