import { useCallback, useEffect, useState } from 'react';
import { notificationsApi } from '../notifications/api';
import type { NotificationSettings } from '../notifications/types';
import { imApi } from './api';
import { WechatSettings } from './WechatSettings';
import './im-settings.css';

export function ImSettingsPanel({ onBusyChange }: { onBusyChange: (busy: boolean) => void }) {
  const [settings, setSettings] = useState<NotificationSettings>();
  const [appId, setAppId] = useState('');
  const [secret, setSecret] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [wechatBusy, setWechatBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState('');
  const feishu = settings?.im.providers.find(provider => provider.provider === 'feishu');
  const wechat = settings?.im.providers.find(provider => provider.provider === 'wechat');
  useEffect(() => {
    let active = true;
    void notificationsApi.notificationSettings().then(next => {
      if (!active) return;
      setSettings(next); setAppId(next.im.providers.find(provider => provider.provider === 'feishu')?.app_id ?? '');
    }).catch(reason => { if (active) setError(String(reason)); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  useEffect(() => { onBusyChange(busy || wechatBusy); }, [busy, wechatBusy, onBusyChange]);
  const refresh = useCallback(async () => { setSettings(await notificationsApi.notificationSettings()); }, []);
  const changed = () => { setError(''); setSaved(''); };
  const save = async (remove = false) => {
    setBusy(true); changed();
    try {
      const next = remove ? await imApi.removeFeishu() : await imApi.saveFeishu(appId.trim(), secret.trim() || undefined);
      setSettings(next); setAppId(next.im.providers.find(provider => provider.provider === 'feishu')?.app_id ?? '');
      setSecret(''); setSaved('IM 配置已保存。');
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  const unavailable = loading || busy || wechatBusy || !settings;
  return <div className="notification-settings-form project-aow-dialog-form">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>IM</h2><p>配置机器人，在飞书或微信中接收通知。</p></div></div>
      {loading && <p role="status">加载配置中…</p>}
      <WechatSettings provider={wechat} disabled={loading || busy || !settings} onChanged={refresh} onBusyChange={setWechatBusy} />
      <form id="im-feishu-settings" onSubmit={event => { event.preventDefault(); void save(); }}>
        <h3>飞书 Bot {feishu && <small className="notification-configured">已配置</small>}</h3>
        <label className="project-aow-dialog-field"><span>App ID</span><input aria-label="飞书 App ID" value={appId} disabled={unavailable} spellCheck={false} autoComplete="off" onChange={event => { setAppId(event.target.value); changed(); }} placeholder="cli_…" required /></label>
        <label className="project-aow-dialog-field"><span>App Secret</span><input aria-label="飞书 App Secret" type="password" value={secret} disabled={unavailable} autoComplete="new-password" onChange={event => { setSecret(event.target.value); changed(); }} placeholder={feishu?.app_id === appId.trim() ? '已保存，留空保留现有密钥' : '填写应用密钥'} required={!feishu || feishu.app_id !== appId.trim()} /></label>
        <p className="project-aow-form-intro">使用企业自建应用，在飞书开发者后台开启机器人能力并发布。消息发送给应用所有者，所有者需在可用范围内。</p>
        <p className="project-aow-form-intro">需要开通“管理应用自身资源”和“以应用的身份发消息”权限。保存后，可在“通知”中选择飞书推送。</p>
      </form>
      {saved && <p role="status">{saved}</p>}
      {error && <div role="alert" className="project-aow-error">{error}</div>}
    </div>
    <footer className="project-aow-dialog-footer">
      {feishu && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void save(true)}>移除飞书配置</button>}
      <button type="submit" form="im-feishu-settings" className="project-aow-dialog-button primary" disabled={unavailable || !appId.trim()}>{busy ? '保存中…' : '保存'}</button>
    </footer>
  </div>;
}
