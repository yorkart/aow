import { useEffect, useRef, useState, type ReactNode } from 'react';
import { imApi } from './api';
import type { ImProviderView, WechatLogin, WechatStatus } from './types';

const failure = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
const polling = (login: WechatLogin) => ['wait', 'scaned', 'scaned_but_redirect'].includes(login.status);
type TestResult = 'idle' | 'sent' | 'confirmed' | 'missing';

function SetupStep({ number, title, complete, current, children }: {
  number: number; title: string; complete: boolean; current: boolean; children: ReactNode;
}) {
  return <li className={`im-wechat-step${complete ? ' complete' : ''}`} aria-current={current ? 'step' : undefined}>
    <div className="im-wechat-step-heading"><span className="im-wechat-step-number" aria-hidden="true">{number}</span>
      <h4>{title}</h4><span className="im-wechat-step-state">{complete ? '已完成' : current ? '当前步骤' : '待完成'}</span></div>
    <div className="im-wechat-step-body">{children}</div>
  </li>;
}

export function WechatSettings({ provider, disabled, onChanged, onBusyChange }: {
  provider?: Extract<ImProviderView, { provider: 'wechat' }>;
  disabled: boolean; onChanged: () => Promise<void>; onBusyChange: (busy: boolean) => void;
}) {
  const [login, setLogin] = useState<WechatLogin>();
  const [cycle, setCycle] = useState(0);
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [testResult, setTestResult] = useState<TestResult>('idle');
  const [testing, setTesting] = useState(false);
  const [statusCycle, setStatusCycle] = useState(0);
  const [statusError, setStatusError] = useState('');
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
    setTestResult('idle');
  }, [provider?.account_id, provider?.user_id]);

  useEffect(() => {
    setConnection(null); setStatusError('');
    if (!provider) return;
    let active = true;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      try {
        const status = await imApi.wechatStatus(controller.signal);
        if (active) {
          setConnection(status.connection); setStatusError('');
        }
      } catch (reason) { if (active) setStatusError(failure(reason)); }
      if (active) timer = setTimeout(() => void check(), 5000);
    };
    void check();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
  }, [provider?.account_id, provider?.user_id, statusCycle]);

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
        if (next.status === 'confirmed') {
          currentId.current = undefined; setStatusCycle(value => value + 1); await onChanged();
        }
        else if (polling(next)) timer = setTimeout(() => void poll(), 1000);
      } catch (reason) { if (active) setError(failure(reason)); }
    };
    void poll();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
    // The loop owns status changes; cycle explicitly resumes after verification/retry.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [login?.id, cycle, onChanged]);

  const operation = async (work: () => Promise<void>) => {
    setBusy(true); onBusyChange(true); setError('');
    try { await work(); } catch (reason) { if (mounted.current) setError(failure(reason)); }
    finally { if (mounted.current) { setBusy(false); onBusyChange(false); } }
  };
  const start = () => operation(async () => {
    if (currentId.current) await imApi.cancelWechat(currentId.current);
    setLogin(undefined); setCode(''); setTestResult('idle');
    const next = await imApi.startWechat();
    if (!mounted.current) { await imApi.cancelWechat(next.id); return; }
    currentId.current = next.id; setLogin(next);
  });
  const cancel = () => operation(async () => {
    if (currentId.current) await imApi.cancelWechat(currentId.current);
    currentId.current = undefined; setLogin(undefined); setCode('');
  });
  const unavailable = disabled || busy;
  const pairing = !!login && (polling(login) || login.status === 'need_verifycode');
  const contextReady = !!provider && !!connection?.context_ready;
  const testSubmitted = testResult !== 'idle';
  const confirmed = testResult === 'confirmed';
  const step = !provider || pairing ? 1 : testSubmitted ? confirmed ? 0 : 4 : contextReady ? 3 : 2;
  const sendTest = () => operation(async () => {
    setTestResult('idle'); setTesting(true);
    try {
      await imApi.testWechat();
      if (mounted.current) setTestResult('sent');
    } catch (reason) {
      if (mounted.current) setStatusCycle(value => value + 1);
      throw reason;
    } finally { if (mounted.current) setTesting(false); }
  });
  return <section className="im-wechat" aria-label="微信 Bot 设置">
    <h3>微信 Bot {provider && <small className="notification-configured">{confirmed ? '已验证' : '已绑定'}</small>}</h3>
    <p className="project-aow-form-intro">使用普通微信扫码绑定，通知发送到扫码账号与 Bot 的私聊。目前不支持普通微信群。</p>
    <ol className="im-wechat-steps" aria-label="微信推送设置步骤">
      <SetupStep number={1} title="扫码绑定微信" complete={!!provider && !pairing} current={step === 1}>
        <p className="project-aow-form-intro">{provider ? `接收账号：${provider.user_id}` : '用接收通知的微信账号扫码，按手机提示完成授权。'}</p>
        <div className="im-actions">
          <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void start()}>{login && login.status !== 'confirmed' ? '重新生成二维码' : provider ? '重新扫码绑定' : '扫码连接微信'}</button>
          {login && login.status !== 'confirmed' && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void cancel()}>关闭扫码</button>}
          {provider && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void operation(async () => {
            await imApi.disconnectWechat(); currentId.current = undefined; setLogin(undefined); setTestResult('idle'); await onChanged();
          })}>解除微信绑定</button>}
        </div>
        {login && login.status !== 'confirmed' && <div className="im-wechat-login">
          {login.qr_image && <img className="im-wechat-qr" src={login.qr_image} alt="微信 Bot 登录二维码" width="256" height="256" />}
          <p role="status">{login.message}</p>
          {login.status === 'need_verifycode' && <form onSubmit={event => {
            event.preventDefault();
            if (unavailable || !code.trim()) return;
            void operation(async () => {
              const next = await imApi.pollWechat(login.id, code.trim());
              setCode(''); setLogin(next);
              if (next.status === 'confirmed') {
                currentId.current = undefined; setStatusCycle(value => value + 1); await onChanged();
              }
              setCycle(value => value + 1);
            });
          }}>
            <label className="project-aow-dialog-field"><span>手机微信显示的数字</span><input aria-label="微信配对码" inputMode="numeric" pattern="[0-9]+" maxLength={16} autoComplete="off" value={code} disabled={unavailable} onChange={event => setCode(event.target.value)} required /></label>
            <button type="submit" className="project-aow-dialog-button" disabled={unavailable || !code.trim()}>确认配对码</button>
          </form>}
          {error && polling(login) && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => { setError(''); setCycle(value => value + 1); }}>重试检查</button>}
        </div>}
      </SetupStep>
      <SetupStep number={2} title="在微信中向 Bot 发一条消息" complete={(contextReady || testSubmitted) && !pairing} current={step === 2}>
        <p className="project-aow-form-intro">绑定后，打开微信里的 Bot 私聊，发送一句“你好”。请保持本页面打开，AoW 收到后会自动更新状态。</p>
        {provider && <p className="im-wechat-progress" role="status">{statusError || connection?.error || (contextReady ? '已收到你的消息，可以发送测试通知。' : '等待接收你在微信中发送的消息…')}</p>}
        {provider && (!contextReady || statusError) && <div className="im-actions"><button type="button" className="project-aow-dialog-button" disabled={unavailable || pairing} onClick={() => setStatusCycle(value => value + 1)}>我已发送，检查状态</button></div>}
      </SetupStep>
      <SetupStep number={3} title="由 AoW 发送测试通知" complete={testSubmitted} current={step === 3}>
        <p className="project-aow-form-intro">{testSubmitted ? '测试请求已提交，请到微信查看“AoW 微信推送测试”。' : '收到你的消息后，点击下方按钮，让 AoW 向当前绑定的微信账号发送一条测试通知。'}</p>
        <div className="im-actions"><button type="button" className="project-aow-dialog-button primary" disabled={unavailable || pairing || !contextReady} onClick={() => void sendTest()}>{testing ? '正在发送…' : '发送微信测试消息'}</button></div>
      </SetupStep>
      <SetupStep number={4} title="确认微信收到通知" complete={confirmed} current={step === 4}>
        <p className="project-aow-form-intro">{confirmed ? '验证完成。请在 Settings → 通知中勾选“微信推送”，并保存通知设置。' : '请以微信中实际出现测试通知为准，收到后点击“我已收到”。'}</p>
        {testResult === 'missing' && <p className="im-wechat-progress" role="status">请确认查看的是本次扫码账号与 Bot 的私聊。向 Bot 再发一条消息，然后重新发送测试通知；仍未收到时，可重新扫码绑定。</p>}
        {testSubmitted && !confirmed && <div className="im-actions">
          <button type="button" className="project-aow-dialog-button primary" disabled={unavailable || pairing} onClick={() => setTestResult('confirmed')}>我已收到</button>
          <button type="button" className="project-aow-dialog-button" disabled={unavailable || pairing} onClick={() => setTestResult('missing')}>还没收到</button>
        </div>}
        {confirmed && <p className="im-wechat-progress im-wechat-verified" role="status">已确认收到微信测试通知。</p>}
      </SetupStep>
    </ol>
    {error && <p className="project-aow-error" role="alert">{error}</p>}
  </section>;
}
