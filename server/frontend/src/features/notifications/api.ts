import type { NotificationSettings, NotificationSettingsUpdate } from './types';
import { aowRequest } from '../../lib/aowRequest';

export const notificationsApi = {
  notificationSettings: () => aowRequest<NotificationSettings>('/api/aow/notification-settings', { cache: 'no-store' }),
  updateNotificationSettings: (input: NotificationSettingsUpdate) => aowRequest<NotificationSettings>('/api/aow/notification-settings', {
    method: 'PUT', body: JSON.stringify(input),
  }),
};
