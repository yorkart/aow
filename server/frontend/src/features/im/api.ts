import { aowRequest } from '../../lib/aowRequest';
import type { NotificationSettings } from '../notifications/types';
import type { WechatLogin, WechatStatus, WechatVerification } from './types';

export const imApi = {
  saveFeishu: (app_id: string, app_secret?: string) => aowRequest<NotificationSettings>('/api/im/feishu', {
    method: 'PUT', body: JSON.stringify({ app_id, app_secret }),
  }),
  removeFeishu: () => aowRequest<NotificationSettings>('/api/im/feishu', { method: 'DELETE' }),
  wechatStatus: (signal?: AbortSignal) => aowRequest<WechatStatus>('/api/im/wechat', { cache: 'no-store', signal }),
  startWechat: () => aowRequest<WechatLogin>('/api/im/wechat/login', { method: 'POST' }),
  pollWechat: (id: string, verify_code?: string, signal?: AbortSignal) => aowRequest<WechatLogin>(`/api/im/wechat/login/${encodeURIComponent(id)}`, {
    method: 'POST', body: JSON.stringify({ verify_code }), signal,
  }),
  cancelWechat: (id: string) => aowRequest<void>(`/api/im/wechat/login/${encodeURIComponent(id)}`, { method: 'DELETE' }),
  disconnectWechat: () => aowRequest<NotificationSettings>('/api/im/wechat', { method: 'DELETE' }),
  testWechat: () => aowRequest<{ message: string; verification: WechatVerification }>('/api/im/wechat/test', { method: 'POST' }),
  wechatReceipt: (test_id: string, received: boolean) => aowRequest<{ verification: WechatVerification }>('/api/im/wechat/test/receipt', {
    method: 'PUT', body: JSON.stringify({ test_id, received }),
  }),
};
