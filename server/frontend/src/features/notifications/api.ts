import type { NotificationSettings, NotificationSettingsUpdate } from './types';
import { aowRequest } from '../../lib/aowRequest';

export const notificationsApi = {
  notificationSettings: () => aowRequest<NotificationSettings>('/api/notification-settings', { cache: 'no-store' }),
  updateNotificationSettings: (input: NotificationSettingsUpdate) => aowRequest<NotificationSettings>('/api/notification-settings', {
    method: 'PUT', body: JSON.stringify(input),
  }),
};
