import type { Language } from '../../lib/i18n';
import type { ImProviderView } from '../im/types';

export type NotificationChannel = 'page' | 'feishu' | 'wechat';

export interface TaskCompletedSettings { enabled: boolean; channels: NotificationChannel[] }

export interface NotificationSettings {
  im: { providers: ImProviderView[] };
  notifications: { language?: Language; agent_task_completed: TaskCompletedSettings; public_base_url?: string };
}

export type NotificationSettingsUpdate =
  | { section: 'im'; providers: { provider: 'feishu'; app_id: string; app_secret?: string }[] }
  | { section: 'notifications'; language?: Language; agent_task_completed: TaskCompletedSettings; public_base_url?: string };
