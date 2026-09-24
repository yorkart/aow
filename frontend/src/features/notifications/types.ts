export type NotificationChannel = 'page' | 'feishu';

export interface TaskCompletedSettings { enabled: boolean; channels: NotificationChannel[] }

export interface NotificationSettings {
  im: { providers: { provider: 'feishu'; app_id: string; secret_configured: boolean }[] };
  notifications: { agent_task_completed: TaskCompletedSettings; public_base_url?: string };
}

export type NotificationSettingsUpdate =
  | { section: 'im'; providers: { provider: 'feishu'; app_id: string; app_secret?: string }[] }
  | { section: 'notifications'; agent_task_completed: TaskCompletedSettings; public_base_url?: string };
