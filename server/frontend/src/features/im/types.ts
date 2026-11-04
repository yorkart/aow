export type ImProviderView =
  | { provider: 'feishu'; app_id: string; secret_configured: boolean }
  | { provider: 'wechat'; account_id: string; user_id: string };

export interface WechatLogin {
  id: string;
  status: 'wait' | 'scaned' | 'need_verifycode' | 'scaned_but_redirect' | 'confirmed' | 'expired' | 'verify_code_blocked';
  qr_image: string | null;
  message: string;
}

export interface WechatStatus {
  connection: { receiving: boolean; context_ready: boolean; error: string | null; verification: WechatVerification | null } | null;
}

export interface WechatVerification {
  test_id: string;
  receipt: 'sent' | 'confirmed' | 'missing';
}
