import { ShieldCheck } from 'lucide-react';
import type { ControlState } from './sessionControl';
import './control.css';

export function ControlStatus({ state, takeover }: ControlState & { takeover: () => void }) {
  if (state === 'claimed') return null;
  return <div className="acp-control-status" role="status">
    <span>{state === 'observing' ? '只读 · 接管后可输入' : state === 'connecting' ? '正在获取会话控制权…' : '控制连接已断开，正在重连…'}</span>
    {state === 'observing' && <button type="button" onClick={takeover}><ShieldCheck size={14} />接管</button>}
  </div>;
}
