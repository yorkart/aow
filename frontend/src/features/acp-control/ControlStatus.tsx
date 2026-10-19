import { LoaderCircle, LockKeyhole, ShieldCheck, WifiOff } from 'lucide-react';
import { ComposerStatus } from '../../components/conversation/ComposerStatus';
import type { ControlState } from './sessionControl';

export function ControlStatus({ state, takeover }: ControlState & { takeover: () => void }) {
  if (state === 'claimed') return null;
  return <ComposerStatus
    icon={state === 'observing' ? <LockKeyhole /> : state === 'connecting' ? <LoaderCircle className="acp-spinner" /> : <WifiOff />}
    title={state === 'observing' ? '接管后可继续对话' : state === 'connecting' ? '正在获取会话控制权…' : '控制连接已断开，正在重连…'}
    description={state === 'observing' ? '当前页面为只读。接管后可在这里输入，其他页面将切换为只读。' : '连接就绪后将自动恢复输入，未发送的草稿会保留。'}
    actions={state === 'observing' && <button type="button" onClick={takeover}><ShieldCheck size={14} />接管</button>}
  />;
}
