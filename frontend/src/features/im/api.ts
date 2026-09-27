import { aowRequest } from '../../lib/aowRequest';
import type { NotificationSettings } from '../notifications/types';
import type { WechatLogin, WechatStatus } from './types';

export const imApi = {
  saveFeishu: (app_id: string, app_secret?: string) => aowRequest<NotificationSettings>('/api/aow/im/feishu', {
    method: 'PUT', body: JSON.stringify({ app_id, app_secret }),
  }),
  removeFeishu: () => aowRequest<NotificationSettings>('/api/aow/im/feishu', { method: 'DELETE' }),
  wechatStatus: (signal?: AbortSignal) => aowRequest<WechatStatus>('/api/aow/im/wechat', { cache: 'no-store', signal }),
  startWechat: () => aowRequest<WechatLogin>('/api/aow/im/wechat/login', { method: 'POST' }),
  pollWechat: (id: string, verify_code?: string, signal?: AbortSignal) => aowRequest<WechatLogin>(`/api/aow/im/wechat/login/${encodeURIComponent(id)}`, {
    method: 'POST', body: JSON.stringify({ verify_code }), signal,
  }),
  cancelWechat: (id: string) => aowRequest<void>(`/api/aow/im/wechat/login/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  disconnectWechat: () => aowRequest<NotificationSettings>('/api/aow/im/wechat', { method: 'DELETE' }),
  testWechat: () => aowRequest<{ message: string }>('/api/aow/im/wechat/test', { method: 'POST' }),
};
