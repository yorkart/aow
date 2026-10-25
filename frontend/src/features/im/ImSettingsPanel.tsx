import { t } from '../../lib/i18n';
import { useCallback, useEffect, useState } from 'react';
import { notificationsApi } from '../notifications/api';
import type { NotificationSettings } from '../notifications/types';
import { imApi } from './api';
import { WechatSettings } from './WechatSettings';
import './im-settings.css';

export function ImSettingsPanel({ onBusyChange, onDirtyChange }: { onBusyChange: (busy: boolean) => void; onDirtyChange?: (dirty: boolean) => void }) {
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
  const dirty = !!settings && (appId !== (feishu?.app_id ?? '') || !!secret);
  useEffect(() => { onDirtyChange?.(dirty); return () => onDirtyChange?.(false); }, [dirty, onDirtyChange]);
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
      setSecret(''); setSaved(t("im.im_configuration_saved"));
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
    finally { setBusy(false); }
  };
  const unavailable = loading || busy || wechatBusy || !settings;
  return <div className="notification-settings-form project-aow-dialog-form">
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div><h2>IM</h2><p>{t("im.configure_bots_to_receive_notifications_in_feishu_or_wechat")}</p></div></div>
      {loading && <p role="status">{t("im.loading_configuration")}</p>}
      <WechatSettings provider={wechat} disabled={loading || busy || !settings} onChanged={refresh} onBusyChange={setWechatBusy} />
      <form id="im-feishu-settings" onSubmit={event => { event.preventDefault(); void save(); }}>
        <h3>{t("automations.feishu_bot")} {feishu && <small className="notification-configured">{t("im.configured")}</small>}</h3>
        <label className="project-aow-dialog-field"><span>App ID</span><input aria-label={t("im.feishu_app_id")} value={appId} disabled={unavailable} spellCheck={false} autoComplete="off" onChange={event => { setAppId(event.target.value); changed(); }} placeholder="cli_…" required /></label>
        <label className="project-aow-dialog-field"><span>App Secret</span><input aria-label={t("im.feishu_app_secret")} type="password" value={secret} disabled={unavailable} autoComplete="new-password" onChange={event => { setSecret(event.target.value); changed(); }} placeholder={feishu?.app_id === appId.trim() ? t("im.saved_leave_blank_to_keep_the_current_secret") : t("im.enter_app_secret")} required={!feishu || feishu.app_id !== appId.trim()} /></label>
        <p className="project-aow-form-intro">{t("im.use_an_enterprise_custom_app_enable_bot_capabilities_and")}</p>
        <p className="project-aow-form-intro">{t("im.enable_permissions_to_manage_the_app_s_own_resources")}</p>
      </form>
      {saved && <p role="status">{saved}</p>}
      {error && <div role="alert" className="project-aow-error">{error}</div>}
    </div>
    <footer className="project-aow-dialog-footer">
      {feishu && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void save(true)}>{t("im.remove_feishu_configuration")}</button>}
      <button type="submit" form="im-feishu-settings" className="project-aow-dialog-button primary" disabled={unavailable || !appId.trim()}>{busy ? t("automations.saving") : t("acp.save")}</button>
    </footer>
  </div>;
}
