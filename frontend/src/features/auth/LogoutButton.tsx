import { useState } from 'react';
import { LogOut } from 'lucide-react';
import { logout } from './authApi';

export function LogoutButton({ disabled = false, compact = false, className }: { disabled?: boolean; compact?: boolean; className?: string }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const leave = async () => {
    if (busy) return;
    setBusy(true);
    setError(undefined);
    try {
      await logout();
      window.location.reload();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '退出登录失败，请重试。');
      setBusy(false);
    }
  };
  return <>
    <button type="button" className={className} disabled={disabled || busy} aria-label="退出登录" title="退出登录" onClick={() => void leave()}>
      <LogOut size={18} />{!compact && <span>{busy ? '正在退出…' : '退出登录'}</span>}
    </button>
    {error && <span role="alert" className="aow-auth-error">{error}</span>}
  </>;
}
