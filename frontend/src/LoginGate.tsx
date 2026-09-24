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

async function login(pin: string): Promise<AuthStatus> {
  const response = await fetch(appUrl('/api/auth/login'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ pin }),
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

  if (loadError) return <AuthScreen title="无法连接 AOW" detail={loadError} retry={() => window.location.reload()} />;
  if (!status) return <AuthScreen title="正在检查登录状态…" detail="" />;
  if (!status.configured) return <AuthScreen title="尚未设置访问 PIN" detail={status.message ?? '请在运行 AOW 的服务器上执行 `aow pin`。'} />;
  if (status.authenticated) return <>{children}</>;
  return <PinLogin onSuccess={() => setStatus({ configured: true, authenticated: true })} />;
}

function PinLogin({ onSuccess }: { onSuccess: () => void }) {
  const [pin, setPin] = useState('');
  const [error, setError] = useState<string>();
  const [submitting, setSubmitting] = useState(false);
  const submittingRef = useRef(false);

  const submit = async (pinToSubmit: string) => {
    if (submittingRef.current) return;
    if (!/^\d{6}$/.test(pinToSubmit)) {
      setError('请输入 6 位数字 PIN。');
      return;
    }
    submittingRef.current = true;
    setSubmitting(true);
    setError(undefined);
    try {
      const result = await login(pinToSubmit);
      if (!result.authenticated) throw new Error('登录未完成，请重试。');
      onSuccess();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '登录失败，请重试。');
      setPin('');
    } finally {
      submittingRef.current = false;
      setSubmitting(false);
    }
  };

  const onPinChange = (value: string) => {
    const nextPin = value.replace(/\D/g, '').slice(0, 6);
    setPin(nextPin);
    setError(undefined);
    if (nextPin.length === 6) void submit(nextPin);
  };

  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    void submit(pin);
  };

  return <main className="aow-auth">
    <section className="aow-auth-card" aria-labelledby="aow-auth-title">
      <div className="aow-auth-mark" aria-hidden="true">A<span /></div>
      <p className="aow-auth-eyebrow">AOW</p>
      <h1 id="aow-auth-title">输入访问 PIN</h1>
      <p>请输入服务器管理员设置的 6 位数字 PIN。</p>
      <form onSubmit={onSubmit}>
        <label htmlFor="aow-pin">PIN 码</label>
        <input id="aow-pin" autoComplete="one-time-code" autoFocus inputMode="numeric" maxLength={6} pattern="[0-9]{6}" placeholder="••••••" type="password" value={pin} disabled={submitting} onChange={(event) => onPinChange(event.target.value)} />
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
      <p className="aow-auth-eyebrow">AOW</p>
      <h1>{title}</h1>
      {detail && <p>{detail}</p>}
      {retry && <button type="button" onClick={retry}>重试</button>}
    </section>
  </main>;
}
