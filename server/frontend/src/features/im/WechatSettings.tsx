import { t } from '../../lib/i18n';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { imApi } from './api';
import type { ImProviderView, WechatLogin, WechatStatus, WechatVerification } from './types';

const failure = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
const polling = (login: WechatLogin) => ['wait', 'scaned', 'scaned_but_redirect'].includes(login.status);

function SetupStep({ number, title, complete, current, children }: {
  number: number; title: string; complete: boolean; current: boolean; children: ReactNode;
}) {
  return <li className={`im-wechat-step${complete ? ' complete' : ''}`} aria-current={current ? 'step' : undefined}>
    <div className="im-wechat-step-heading"><span className="im-wechat-step-number" aria-hidden="true">{number}</span>
      <h4>{title}</h4><span className="im-wechat-step-state">{complete ? t("aow.completed") : current ? t("im.current_step") : t("im.pending")}</span></div>
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
  const [testing, setTesting] = useState(false);
  const [statusCycle, setStatusCycle] = useState(0);
  const [statusError, setStatusError] = useState('');
  const [connection, setConnection] = useState<WechatStatus['connection']>(null);
  const mounted = useRef(true);
  const currentId = useRef<string | undefined>(undefined);
  const statusVersion = useRef(0);
  const operating = useRef(false);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (currentId.current) void imApi.cancelWechat(currentId.current).catch(() => {});
    };
  }, []);

  useEffect(() => {
    setConnection(null);
  }, [provider?.account_id, provider?.user_id]);

  useEffect(() => {
    setStatusError('');
    if (!provider) return;
    let active = true;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      const version = statusVersion.current;
      try {
        if (!operating.current) {
          const status = await imApi.wechatStatus(controller.signal);
          if (active && !operating.current && version === statusVersion.current) {
            setConnection(status.connection); setStatusError('');
          }
        }
      } catch (reason) { if (active && version === statusVersion.current) setStatusError(failure(reason)); }
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
          currentId.current = undefined; statusVersion.current++; setConnection(null);
          setStatusCycle(value => value + 1); await onChanged();
        }
        else if (polling(next)) timer = setTimeout(() => void poll(), 1000);
      } catch (reason) { if (active) setError(failure(reason)); }
    };
    void poll();
    return () => { active = false; controller.abort(); clearTimeout(timer); };
    // The loop owns status changes; cycle explicitly resumes after verification/retry.
  }, [login?.id, cycle, onChanged]);

  const operation = async (work: () => Promise<void>) => {
    operating.current = true; statusVersion.current++;
    setBusy(true); onBusyChange(true); setError('');
    try { await work(); } catch (reason) {
      if (mounted.current) { setError(failure(reason)); setStatusCycle(value => value + 1); }
    } finally {
      operating.current = false; statusVersion.current++;
      if (mounted.current) { setBusy(false); onBusyChange(false); }
    }
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
  const pairing = !!login && (polling(login) || login.status === 'need_verifycode');
  const contextReady = !!provider && !!connection?.context_ready;
  const verification = pairing ? null : connection?.verification;
  const testResult = verification?.receipt ?? 'idle';
  const testSubmitted = testResult !== 'idle';
  const confirmed = testResult === 'confirmed';
  const step = !provider || pairing ? 1 : testSubmitted ? confirmed ? 0 : 4 : contextReady ? 3 : 2;
  const updateVerification = (verification: WechatVerification | null) => {
    setConnection(current => current && { ...current, verification });
  };
  const sendTest = () => operation(async () => {
    updateVerification(null); setTesting(true);
    try {
      const result = await imApi.testWechat();
      if (mounted.current) updateVerification(result.verification);
    } finally { if (mounted.current) setTesting(false); }
  });
  const receipt = (received: boolean) => operation(async () => {
    if (!verification) return;
    const result = await imApi.wechatReceipt(verification.test_id, received);
    if (mounted.current) updateVerification(result.verification);
  });
  return <section className="im-wechat" aria-label={t("im.wechat_bot_settings")}>
    <h3>{t("automations.wechat_bot")} {provider && <small className="notification-configured">{confirmed ? t("im.verified") : t("im.linked")}</small>}</h3>
    <p className="project-aow-form-intro">{t("im.scan_with_wechat_to_link_your_account_notifications_go")}</p>
    <ol className="im-wechat-steps" aria-label={t("im.wechat_push_setup_steps")}>
      <SetupStep number={1} title={t("im.scan_to_link_wechat")} complete={!!provider && !pairing} current={step === 1}>
        <p className="project-aow-form-intro">{provider ? t("im.recipient_account", { value0: provider.user_id }) : t("im.scan_with_the_wechat_account_that_will_receive_notifications")}</p>
        <div className="im-actions">
          <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void start()}>{login && login.status !== 'confirmed' ? t("im.generate_new_qr_code") : provider ? t("im.scan_again_to_link") : t("im.scan_to_connect_wechat")}</button>
          {login && login.status !== 'confirmed' && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void cancel()}>{t("im.close_qr_code")}</button>}
          {provider && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void operation(async () => {
            await imApi.disconnectWechat(); currentId.current = undefined; setLogin(undefined); setConnection(null); await onChanged();
          })}>{t("im.unlink_wechat")}</button>}
        </div>
        {login && login.status !== 'confirmed' && <div className="im-wechat-login">
          {login.qr_image && <img className="im-wechat-qr" src={login.qr_image} alt={t("im.wechat_bot_login_qr_code")} width="256" height="256" />}
          <p role="status">{login.message}</p>
          {login.status === 'need_verifycode' && <form onSubmit={event => {
            event.preventDefault();
            if (unavailable || !code.trim()) return;
            void operation(async () => {
              const next = await imApi.pollWechat(login.id, code.trim());
              setCode(''); setLogin(next);
              if (next.status === 'confirmed') {
                currentId.current = undefined; setConnection(null); setStatusCycle(value => value + 1); await onChanged();
              }
              setCycle(value => value + 1);
            });
          }}>
            <label className="project-aow-dialog-field"><span>{t("im.number_displayed_in_wechat_on_your_phone")}</span><input aria-label={t("im.wechat_pairing_code")} inputMode="numeric" pattern="[0-9]+" maxLength={16} autoComplete="off" value={code} disabled={unavailable} onChange={event => setCode(event.target.value)} required /></label>
            <button type="submit" className="project-aow-dialog-button" disabled={unavailable || !code.trim()}>{t("im.confirm_pairing_code")}</button>
          </form>}
          {error && polling(login) && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => { setError(''); setCycle(value => value + 1); }}>{t("im.retry_check")}</button>}
        </div>}
      </SetupStep>
      <SetupStep number={2} title={t("im.send_a_message_to_the_bot_in_wechat")} complete={(contextReady || testSubmitted) && !pairing} current={step === 2}>
        <p className="project-aow-form-intro">{t("im.after_linking_open_the_bot_s_private_chat_and")}</p>
        {provider && <p className="im-wechat-progress" role="status">{statusError || connection?.error || (contextReady ? t("im.message_received_you_can_send_a_test_notification") : t("im.waiting_for_your_message_from_wechat"))}</p>}
        {provider && (!contextReady || statusError) && <div className="im-actions"><button type="button" className="project-aow-dialog-button" disabled={unavailable || pairing} onClick={() => setStatusCycle(value => value + 1)}>{t("im.i_sent_it_check_status")}</button></div>}
      </SetupStep>
      <SetupStep number={3} title={t("im.send_a_test_notification_from_aow")} complete={testSubmitted} current={step === 3}>
        <p className="project-aow-form-intro">{testSubmitted ? t("im.test_request_submitted_check_wechat_for_aow_wechat_push") : t("im.after_your_message_is_received_click_below_to_send")}</p>
        <div className="im-actions"><button type="button" className="project-aow-dialog-button primary" disabled={unavailable || pairing || !contextReady} onClick={() => void sendTest()}>{testing ? t("im.sending") : t("im.send_wechat_test_message")}</button></div>
      </SetupStep>
      <SetupStep number={4} title={t("im.confirm_notification_received_in_wechat")} complete={confirmed} current={step === 4}>
        <p className="project-aow-form-intro">{confirmed ? t("im.verification_complete_and_saved_select_wechat_push_in_settings") : t("im.confirm_the_test_notification_actually_appears_in_wechat_then")}</p>
        {testResult === 'missing' && <p className="im-wechat-progress" role="status">{t("im.check_the_private_chat_between_the_scanned_account_and")}</p>}
        {testSubmitted && !confirmed && <div className="im-actions">
          <button type="button" className="project-aow-dialog-button primary" disabled={unavailable || pairing} onClick={() => void receipt(true)}>{t("im.i_received_it")}</button>
          <button type="button" className="project-aow-dialog-button" disabled={unavailable || pairing} onClick={() => void receipt(false)}>{t("im.not_received_yet")}</button>
        </div>}
        {confirmed && <p className="im-wechat-progress im-wechat-verified" role="status">{t("im.wechat_test_notification_receipt_confirmed")}</p>}
      </SetupStep>
    </ol>
    {error && <p className="project-aow-error" role="alert">{error}</p>}
  </section>;
}
