import { appBaseUrl } from '../../lib/basePath';
import { useEffect, useState } from 'react';
import { notificationsApi } from './api';
import type { NotificationChannel, NotificationSettings, NotificationSettingsUpdate, TaskCompletedSettings } from './types';
import './notification-settings.css';
import { ImSettingsPanel } from '../im/ImSettingsPanel';

export function NotificationSettingsPanel({ section, onBusyChange }: {
  section: 'im' | 'notifications'; onBusyChange: (busy: boolean) => void;
}) {
  return section === 'im' ? <ImSettingsPanel onBusyChange={onBusyChange} /> : <NotificationPreferences onBusyChange={onBusyChange} />;
}

function NotificationPreferences({ onBusyChange }: { onBusyChange: (busy: boolean) => void }) {
  const [settings, setSettings] = useState<NotificationSettings>();
  const [task, setTask] = useState<TaskCompletedSettings>({ enabled: true, channels: ['page'] });
  const [publicBaseUrl, setPublicBaseUrl] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState('');
  const feishu = settings?.im.providers.find(provider => provider.provider === 'feishu');
  const wechat = settings?.im.providers.find(provider => provider.provider === 'wechat');

  useEffect(() => {
    let active = true;
    void notificationsApi.notificationSettings().then(next => {
      if (!active) return;
      setSettings(next);
      setTask(next.notifications.agent_task_completed);
      setPublicBaseUrl(next.notifications.public_base_url ?? '');
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
      setSaved('通知配置已保存，即时生效。');
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
    if (!noChannels) void save({ section: 'notifications', agent_task_completed: task, public_base_url: publicBaseUrl.trim() });
  }}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div>
        <h2>通知</h2>
        <p>选择 Agent 任务完成后的通知方式。</p>
      </div></div>
      {loading && <p role="status">加载配置中…</p>}
      <>
        <label className="project-aow-dialog-field"><span>AOW 访问地址</span><input type="url" aria-label="AOW 访问地址" value={publicBaseUrl} disabled={unavailable} spellCheck={false} placeholder="https://aow.example.com" onChange={event => { setPublicBaseUrl(event.target.value); changed(); }} /></label>
        <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => { setPublicBaseUrl(appBaseUrl()); changed(); }}>使用当前访问地址</button>
        <p className="project-aow-form-intro">用于 IM 通知中的 Tab 链接，请填写接收设备可访问的部署入口地址，包含端口和子目录（如有）。留空时只显示 Tab 名称。</p>
        <label className="project-aow-dialog-checkbox notification-checkbox notification-master"><input type="checkbox" checked={task.enabled} disabled={unavailable} onChange={event => {
          changed(); setTask(previous => ({ ...previous, enabled: event.target.checked,
            channels: event.target.checked && !previous.channels.length ? ['page'] : previous.channels }));
        }} /><span>Agent 任务完成</span></label>
        {settings && task.enabled && <fieldset disabled={unavailable} className="notification-channels">
          <legend>通知方式（至少选择一项）</legend>
          <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('page')} onChange={event => toggleChannel('page', event.target.checked)} /><span>页面提示</span></label>
          {feishu && <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('feishu')} onChange={event => toggleChannel('feishu', event.target.checked)} /><span>飞书推送</span></label>}
          {wechat && <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('wechat')} onChange={event => toggleChannel('wechat', event.target.checked)} /><span>微信推送</span></label>}
          {!feishu && !wechat && <p className="project-aow-form-intro">在 IM 中配置飞书或微信 Bot 后，可选择 IM 推送。</p>}
          {noChannels && <p role="alert" className="project-aow-error">至少选择一种通知方式。</p>}
        </fieldset>}
        <p className="project-aow-form-intro">通知包含项目、Tab 名称和触发的 Session ID。IM 推送在网页关闭后仍可发送。</p>
      </>
      {saved && <p role="status">{saved}</p>}
      {error && <div role="alert" className="project-aow-error">{error}</div>}
    </div>
    <footer className="project-aow-dialog-footer">
      <button type="submit" className="project-aow-dialog-button primary" disabled={unavailable || noChannels}>{busy ? '保存中…' : '保存'}</button>
    </footer>
  </form>;
}
