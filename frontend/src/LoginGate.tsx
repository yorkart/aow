import { appUrl } from './lib/basePath';
import { FormEvent, ReactNode, useEffect, useRef, useState } from 'react';

interface AuthStatus {
  configured: boolean;
  authenticated: boolean;
  message?: string;
}

async function authStatus(): Promise<AuthStatus> {
  const response = await fetch(appUrl('/api/auth/status'), { cache: 'no-store' });
  if (!response.ok) throw new Error(`无法读取登录状态（HTTP ${response.status}）`);
  return response.json() as Promise<AuthStatus>;
}

async function login(username: string, password: string): Promise<AuthStatus> {
  const response = await fetch(appUrl('/api/auth/login'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  const payload = await response.json().catch(() => null) as { message?: string } | null;
  if (!response.ok) throw new Error(payload?.message ?? `登录失败（HTTP ${response.status}）`);
  return payload as AuthStatus;
}

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

  if (loadError) return <AuthScreen title="无法连接 AoW" detail={loadError} retry={() => window.location.reload()} />;
  if (!status) return <AuthScreen title="正在检查登录状态…" detail="" />;
  if (!status.configured) return <AuthScreen title="尚未设置登录账户" detail={status.message ?? '请在运行 AoW 的服务器上执行 `aow account`。'} />;
  if (status.authenticated) return <>{children}</>;
  return <AccountLogin onSuccess={() => setStatus({ configured: true, authenticated: true })} />;
}

function AccountLogin({ onSuccess }: { onSuccess: () => void }) {
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
      const result = await login(username, password);
      if (!result.authenticated) throw new Error('登录未完成，请重试。');
      onSuccess();
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

  return <main className="aow-auth">
    <section className="aow-auth-card" aria-labelledby="aow-auth-title">
      <div className="aow-auth-mark" aria-hidden="true">A<span /></div>
      <p className="aow-auth-eyebrow">AoW</p>
      <h1 id="aow-auth-title">登录 AoW</h1>
      <p>请输入首次安装时设置的账号和密码。</p>
      <form onSubmit={onSubmit}>
        <label htmlFor="aow-username">账号</label>
        <input id="aow-username" name="username" autoComplete="username" autoFocus autoCapitalize="none" spellCheck={false} placeholder="请输入账号" type="text" value={username} required disabled={submitting} onChange={(event) => { setUsername(event.target.value); setError(undefined); }} />
        <label htmlFor="aow-password">密码</label>
        <input id="aow-password" name="password" autoComplete="current-password" placeholder="请输入密码" type="password" value={password} required disabled={submitting} onChange={(event) => { setPassword(event.target.value); setError(undefined); }} />
        {error && <p className="aow-auth-error" role="alert">{error}</p>}
        <button type="submit" disabled={submitting}>{submitting ? '正在验证…' : '登录'}</button>
      </form>
    </section>
  </main>;
}

function AuthScreen({ title, detail, retry }: { title: string; detail: string; retry?: () => void }) {
  return <main className="aow-auth">
    <section className="aow-auth-card aow-auth-message" aria-live="polite">
      <div className="aow-auth-mark" aria-hidden="true">A<span /></div>
      <p className="aow-auth-eyebrow">AoW</p>
      <h1>{title}</h1>
      {detail && <p>{detail}</p>}
      {retry && <button type="button" onClick={retry}>重试</button>}
    </section>
  </main>;
}
