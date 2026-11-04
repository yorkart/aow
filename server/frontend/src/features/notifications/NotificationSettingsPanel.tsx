import { t, languages, type Language } from '../../lib/i18n';
import { AowSelect } from '../../components/AowSelect';
import { appBaseUrl } from '../../lib/basePath';
import { useEffect, useState } from 'react';
import { notificationsApi } from './api';
import type { NotificationChannel, NotificationSettings, NotificationSettingsUpdate, TaskCompletedSettings } from './types';
import './notification-settings.css';
import { ImSettingsPanel } from '../im/ImSettingsPanel';

export function NotificationSettingsPanel({ section, onBusyChange, onDirtyChange }: {
  section: 'im' | 'notifications'; onBusyChange: (busy: boolean) => void; onDirtyChange?: (dirty: boolean) => void;
}) {
  return section === 'im' ? <ImSettingsPanel onBusyChange={onBusyChange} onDirtyChange={onDirtyChange} /> : <NotificationPreferences onBusyChange={onBusyChange} onDirtyChange={onDirtyChange} />;
}

function NotificationPreferences({ onBusyChange, onDirtyChange }: { onBusyChange: (busy: boolean) => void; onDirtyChange?: (dirty: boolean) => void }) {
  const [settings, setSettings] = useState<NotificationSettings>();
  const [task, setTask] = useState<TaskCompletedSettings>({ enabled: true, channels: ['page'] });
  const [publicBaseUrl, setPublicBaseUrl] = useState('');
  const [language, setLanguage] = useState<Language>('en-US');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState('');
  const feishu = settings?.im.providers.find(provider => provider.provider === 'feishu');
  const wechat = settings?.im.providers.find(provider => provider.provider === 'wechat');
  const dirty = !!settings && (JSON.stringify(task) !== JSON.stringify(settings.notifications.agent_task_completed)
    || publicBaseUrl !== (settings.notifications.public_base_url ?? '') || language !== (settings.notifications.language ?? 'en-US'));
  useEffect(() => { onDirtyChange?.(dirty); return () => onDirtyChange?.(false); }, [dirty, onDirtyChange]);

  useEffect(() => {
    let active = true;
    void notificationsApi.notificationSettings().then(next => {
      if (!active) return;
      setSettings(next);
      setTask(next.notifications.agent_task_completed);
      setPublicBaseUrl(next.notifications.public_base_url ?? '');
      setLanguage(next.notifications.language ?? 'en-US');
    }).catch(reason => { if (active) setError(reason instanceof Error ? reason.message : String(reason)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  const changed = () => { setError(''); setSaved(''); };
  const save = async (update: NotificationSettingsUpdate) => {
    setBusy(true); onBusyChange(true); changed();
    try {
      const next = await notificationsApi.updateNotificationSettings(update);
      setSettings(next);
      setTask(next.notifications.agent_task_completed);
      setPublicBaseUrl(next.notifications.public_base_url ?? '');
      setLanguage(next.notifications.language ?? 'en-US');
      setSaved(t("notifications.notification_configuration_saved_and_applied_immediately"));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally { setBusy(false); onBusyChange(false); }
  };
  const toggleChannel = (channel: NotificationChannel, checked: boolean) => {
    changed();
    setTask(previous => ({ ...previous, channels: checked
      ? [...previous.channels, channel] : previous.channels.filter(value => value !== channel) }));
  };
  const unavailable = loading || busy || !settings;
  const noChannels = task.enabled && task.channels.length === 0;

  return <form className="notification-settings-form project-aow-dialog-form" onSubmit={event => {
    event.preventDefault();
    if (!noChannels) void save({ section: 'notifications', language, agent_task_completed: task, public_base_url: publicBaseUrl.trim() });
  }}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div>
        <h2>{t("notifications.notifications_2")}</h2>
        <p>{t("notifications.choose_how_to_be_notified_when_agent_tasks_finish")}</p>
      </div></div>
      {loading && <p role="status">{t("im.loading_configuration")}</p>}
      <>
        <label className="project-aow-dialog-field"><span>{t('language.notificationLanguage')}</span><AowSelect label={t('language.notificationLanguage')} disabled={unavailable} value={language} options={languages.map(item => ({ value: item.id, label: item.name }))} onChange={value => { setLanguage(value as Language); changed(); }} /></label>
        <p className="project-aow-form-intro">{t('language.notificationDescription')}</p>
        <label className="project-aow-dialog-field"><span>{t("notifications.aow_url")}</span><input type="url" aria-label={t("notifications.aow_url")} value={publicBaseUrl} disabled={unavailable} spellCheck={false} placeholder="https://aow.example.com" onChange={event => { setPublicBaseUrl(event.target.value); changed(); }} /></label>
        <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => { setPublicBaseUrl(appBaseUrl()); changed(); }}>{t("notifications.use_current_url")}</button>
        <p className="project-aow-form-intro">{t("notifications.used_for_tab_links_in_im_notifications_enter_a")}</p>
        <label className="project-aow-dialog-checkbox notification-checkbox notification-master"><input type="checkbox" checked={task.enabled} disabled={unavailable} onChange={event => {
          changed(); setTask(previous => ({ ...previous, enabled: event.target.checked,
            channels: event.target.checked && !previous.channels.length ? ['page'] : previous.channels }));
        }} /><span>{t("notifications.agent_task_completion")}</span></label>
        {settings && task.enabled && <fieldset disabled={unavailable} className="notification-channels">
          <legend>{t("notifications.notification_methods_choose_at_least_one")}</legend>
          <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('page')} onChange={event => toggleChannel('page', event.target.checked)} /><span>{t("notifications.in_page_notification")}</span></label>
          {feishu && <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('feishu')} onChange={event => toggleChannel('feishu', event.target.checked)} /><span>{t("notifications.feishu_push")}</span></label>}
          {wechat && <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('wechat')} onChange={event => toggleChannel('wechat', event.target.checked)} /><span>{t("notifications.wechat_push")}</span></label>}
          {!feishu && !wechat && <p className="project-aow-form-intro">{t("notifications.configure_a_feishu_or_wechat_bot_in_im_to")}</p>}
          {noChannels && <p role="alert" className="project-aow-error">{t("notifications.select_at_least_one_notification_method")}</p>}
        </fieldset>}
        <p className="project-aow-form-intro">{t("notifications.notifications_include_the_project_tab_name_and_triggering_session")}</p>
      </>
      {saved && <p role="status">{saved}</p>}
      {error && <div role="alert" className="project-aow-error">{error}</div>}
    </div>
    <footer className="project-aow-dialog-footer">
      <button type="submit" className="project-aow-dialog-button primary" disabled={unavailable || noChannels}>{busy ? t("automations.saving") : t("acp.save")}</button>
    </footer>
  </form>;
}
