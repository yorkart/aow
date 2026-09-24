import { appBaseUrl } from '../../lib/basePath';
import { useEffect, useState } from 'react';
import { notificationsApi } from './api';
import type { NotificationChannel, NotificationSettings, NotificationSettingsUpdate, TaskCompletedSettings } from './types';
import './notification-settings.css';

export function NotificationSettingsPanel({ section, onBusyChange }: {
  section: 'im' | 'notifications'; onBusyChange: (busy: boolean) => void;
}) {
  const [settings, setSettings] = useState<NotificationSettings>();
  const [appId, setAppId] = useState('');
  const [secret, setSecret] = useState('');
  const [task, setTask] = useState<TaskCompletedSettings>({ enabled: true, channels: ['page'] });
  const [publicBaseUrl, setPublicBaseUrl] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState('');
  const feishu = settings?.im.providers.find(provider => provider.provider === 'feishu');

  useEffect(() => {
    let active = true;
    void notificationsApi.notificationSettings().then(next => {
      if (!active) return;
      setSettings(next);
      setAppId(next.im.providers.find(provider => provider.provider === 'feishu')?.app_id ?? '');
      setTask(next.notifications.agent_task_completed);
      setPublicBaseUrl(next.notifications.public_base_url ?? '');
    }).catch(reason => { if (active) setError(reason instanceof Error ? reason.message : String(reason)); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);

  useEffect(() => { setError(''); setSaved(''); }, [section]);

  const changed = () => { setError(''); setSaved(''); };
  const save = async (update: NotificationSettingsUpdate) => {
    setBusy(true); onBusyChange(true); changed();
    try {
      const next = await notificationsApi.updateNotificationSettings(update);
      setSettings(next);
      if (update.section === 'im') {
        setAppId(next.im.providers.find(provider => provider.provider === 'feishu')?.app_id ?? '');
        setSecret('');
      } else {
        setTask(next.notifications.agent_task_completed);
        setPublicBaseUrl(next.notifications.public_base_url ?? '');
      }
      setSaved(update.section === 'im' ? 'IM 配置已保存。' : '通知配置已保存，即时生效。');
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
    if (section === 'im') void save({ section: 'im', providers: [{ provider: 'feishu', app_id: appId.trim(), ...(secret.trim() ? { app_secret: secret.trim() } : {}) }] });
    else if (!noChannels) void save({ section: 'notifications', agent_task_completed: task, public_base_url: publicBaseUrl.trim() });
  }}>
    <div className="project-aow-dialog-body">
      <div className="project-aow-settings-heading"><div>
        <h2>{section === 'im' ? 'IM' : '通知'}</h2>
        <p>{section === 'im' ? '配置飞书应用机器人，消息发送给应用所有者。' : '选择 Agent 任务完成后的通知方式。'}</p>
      </div></div>
      {loading && <p role="status">加载配置中…</p>}
      {section === 'im' ? <>
        <h3>飞书 Bot {feishu && <small className="notification-configured">已配置</small>}</h3>
        <label className="project-aow-dialog-field"><span>App ID</span><input aria-label="飞书 App ID" value={appId} disabled={unavailable} spellCheck={false} autoComplete="off" onChange={event => { setAppId(event.target.value); changed(); }} placeholder="cli_…" required /></label>
        <label className="project-aow-dialog-field"><span>App Secret</span><input aria-label="飞书 App Secret" type="password" value={secret} disabled={unavailable} autoComplete="new-password" onChange={event => { setSecret(event.target.value); changed(); }} placeholder={feishu?.app_id === appId.trim() ? '已保存，留空保留现有密钥' : '填写应用密钥'} required={!feishu || feishu.app_id !== appId.trim()} /></label>
        <p className="project-aow-form-intro">使用企业自建应用，在飞书开发者后台开启机器人能力并发布。应用所有者需在可用范围内。</p>
        <p className="project-aow-form-intro">需要开通“管理应用自身资源”和“以应用的身份发消息”权限。保存后，可在“通知”中选择飞书推送。</p>
      </> : <>
        <label className="project-aow-dialog-field"><span>AOW 访问地址</span><input type="url" aria-label="AOW 访问地址" value={publicBaseUrl} disabled={unavailable} spellCheck={false} placeholder="https://aow.example.com" onChange={event => { setPublicBaseUrl(event.target.value); changed(); }} /></label>
        <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => { setPublicBaseUrl(appBaseUrl()); changed(); }}>使用当前访问地址</button>
        <p className="project-aow-form-intro">用于飞书通知中的 Tab 链接，请填写接收设备可访问的部署入口地址，包含端口和子目录（如有）。留空时飞书只显示 Tab 名称。</p>
        <label className="project-aow-dialog-checkbox notification-checkbox notification-master"><input type="checkbox" checked={task.enabled} disabled={unavailable} onChange={event => {
          changed(); setTask(previous => ({ ...previous, enabled: event.target.checked,
            channels: event.target.checked && !previous.channels.length ? ['page'] : previous.channels }));
        }} /><span>Agent 任务完成</span></label>
        {task.enabled && <fieldset disabled={unavailable} className="notification-channels">
          <legend>通知方式（至少选择一项）</legend>
          <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('page')} onChange={event => toggleChannel('page', event.target.checked)} /><span>页面提示</span></label>
          {feishu && <label className="project-aow-dialog-checkbox notification-checkbox"><input type="checkbox" checked={task.channels.includes('feishu')} onChange={event => toggleChannel('feishu', event.target.checked)} /><span>飞书推送</span></label>}
          {!feishu && <p className="project-aow-form-intro">配置 IM 中的飞书 Bot 后，可选择飞书推送。</p>}
          {noChannels && <p role="alert" className="project-aow-error">至少选择一种通知方式。</p>}
        </fieldset>}
        <p className="project-aow-form-intro">通知包含项目、Tab 名称和触发的 Session ID。飞书推送在网页关闭后仍可发送。</p>
      </>}
      {saved && <p role="status">{saved}</p>}
      {error && <div role="alert" className="project-aow-error">{error}</div>}
    </div>
    <footer className="project-aow-dialog-footer">
      {section === 'im' && feishu && <button type="button" className="project-aow-dialog-button" disabled={unavailable} onClick={() => void save({ section: 'im', providers: [] })}>移除飞书配置</button>}
      <button type="submit" className="project-aow-dialog-button primary" disabled={unavailable || (section === 'notifications' ? noChannels : !appId.trim())}>{busy ? '保存中…' : '保存'}</button>
    </footer>
  </form>;
}
