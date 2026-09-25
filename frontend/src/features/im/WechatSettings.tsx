import { useEffect, useRef, useState } from 'react';
import { imApi } from './api';
import type { ImProviderView, WechatLogin, WechatStatus } from './types';

const failure = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
const polling = (login: WechatLogin) => ['wait', 'scaned', 'scaned_but_redirect'].includes(login.status);

export function WechatSettings({ provider, disabled, onChanged, onBusyChange }: {
  provider?: Extract<ImProviderView, { provider: 'wechat' }>;
  disabled: boolean; onChanged: () => Promise<void>; onBusyChange: (busy: boolean) => void;
}) {
  const [login, setLogin] = useState<WechatLogin>();
  const [cycle, setCycle] = useState(0);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [connection, setConnection] = useState<WechatStatus['connection']>(null);
  const mounted = useRef(true);
  const currentId = useRef<string | undefined>(undefined);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (currentId.current) void imApi.cancelWechat(currentId.current).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!provider) { setConnection(null); return; }
    let active = true;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      try {
        const status = await imApi.wechatStatus(controller.signal);
        if (active) setConnection(status.connection);
      } catch { /* Keep binding visible while status is temporarily unavailable. */ }
      if (active) timer = setTimeout(() => void check(), 5000);
    };
    void check();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [provider?.account_id]);

  useEffect(() => {
    if (!login || !polling(login)) return;
    let active = true;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const id = login.id;
    const poll = async () => {
      try {
        const next = await imApi.pollWechat(id, undefined, controller.signal);
        if (!active) return;
        setLogin(next); setError('');
        if (next.status === 'confirmed') await onChanged();
        else if (polling(next)) timer = setTimeout(() => void poll(), 1000);
      } catch (reason) { if (active) setError(failure(reason)); }
    };
    void poll();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
    // The loop owns status changes; cycle explicitly resumes after verification/retry.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [login?.id, cycle, onChanged]);

  const operation = async (work: () => Promise<void>) => {
    setBusy(true); onBusyChange(true); setError(''); setNotice('');
    try { await work(); } catch (reason) { if (mounted.current) setError(failure(reason)); }
    finally { if (mounted.current) { setBusy(false); onBusyChange(false); } }
  };
  const start = () => operation(async () => {
    if (currentId.current) await imApi.cancelWechat(currentId.current);
    setLogin(undefined); setCode('');
    const next = await imApi.startWechat();
    if (!mounted.current) { await imApi.cancelWechat(next.id); return; }
    currentId.current = next.id; setLogin(next);
  });
  const cancel = () => operation(async () => {
    if (currentId.current) await imApi.cancelWechat(currentId.current);
    currentId.current = undefined; setLogin(undefined); setCode('');
  });
  const unavailable = disabled || busy;
  return <section className="im-wechat" aria-label="微信 Bot 设置">
    <h3>微信 Bot {provider && <small className="notification-configured">已绑定</small>}</h3>
    <p className="project-aow-form-intro">使用普通微信扫码绑定，通知发送到扫码账号与 Bot 的私聊。目前不支持普通微信群。</p>
    {provider && <>
      <p className="project-aow-form-intro">接收账号：{provider.user_id}</p>
      <p className="project-aow-form-intro">{connection?.error ?? (connection?.context_ready ? '已收到微信会话，可发送通知。' : '可先发送测试消息；若发送失败，请向微信 Bot 发一条消息后重试。')}</p>
      <div className="im-actions">
        <button type="button" className="project-aow-dialog-button" disabled={unavailable || !!login && polling(login)} onClick={() => void operation(async () => setNotice((await imApi.testWechat()).message))}>发送微信测试消息</button>
        <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void operation(async () => {
          await imApi.disconnectWechat(); currentId.current = undefined; setLogin(undefined); await onChanged();
        })}>解除微信绑定</button>
      </div>
    </>}
    <div className="im-actions">
      <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void start()}>{login ? '重新生成二维码' : provider ? '重新扫码绑定' : '扫码连接微信'}</button>
      {login && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void cancel()}>关闭扫码</button>}
    </div>
    {login && <div className="im-wechat-login">
      {login.qr_image && <img className="im-wechat-qr" src={login.qr_image} alt="微信 Bot 登录二维码" width="256" height="256" />}
      <p role="status">{login.message}</p>
      {login.status === 'need_verifycode' && <form onSubmit={event => {
        event.preventDefault();
        if (unavailable || !code.trim()) return;
        void operation(async () => {
          const next = await imApi.pollWechat(login.id, code.trim());
          setCode(''); setLogin(next);
          if (next.status === 'confirmed') await onChanged();
          setCycle(value => value + 1);
        });
      }}>
        <label className="project-aow-dialog-field"><span>手机微信显示的数字</span><input aria-label="微信配对码" inputMode="numeric" pattern="[0-9]+" maxLength={16} autoComplete="off" value={code} disabled={unavailable} onChange={event => setCode(event.target.value)} required /></label>
        <button type="submit" className="project-aow-dialog-button" disabled={unavailable || !code.trim()}>确认配对码</button>
      </form>}
      {error && polling(login) && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => { setError(''); setCycle(value => value + 1); }}>重试检查</button>}
    </div>}
    {notice && <p role="status">{notice}</p>}
    {error && <p className="project-aow-error" role="alert">{error}</p>}
  </section>;
}
