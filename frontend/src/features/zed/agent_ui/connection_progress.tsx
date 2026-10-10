import { useEffect, useState } from 'react';
import type { ConnectionStatus } from '../types';

const phases: Record<string, string> = {
  queued: '正在等待其他 Agent 安装完成…', configuration: '正在读取配置…', registry: '正在读取 ACP Registry…',
  runtime: '正在检查 Node.js 和 npm…', installing: '正在下载并安装 Agent 依赖…',
  download: '正在下载 Agent…', extract: '正在解压 Agent…', starting: '正在启动 Agent…', initializing: '正在建立 ACP 连接…',
};
export function ConnectionProgress({ status, message }: { status?: ConnectionStatus; message: string }) {
  const [seconds, setSeconds] = useState(0);
  useEffect(() => { const started = Date.now(); const timer = window.setInterval(() => setSeconds(Math.floor((Date.now() - started) / 1000)), 1000); return () => window.clearInterval(timer); }, []);
  return <div className="zed-status" role="status">
    <span>{status ? phases[status.phase] || message : message} · 已等待 {Math.max(seconds, status?.elapsed_seconds ?? 0)} 秒</span>
    {status?.detail && <p>网络连接失败（{status.detail}），正在等待重试。可在 Settings → Environment 的 server.env 中配置服务代理，并重启 Web 服务。</p>}
    {status?.phase === 'installing' && !status.detail && <p>首次安装需要下载依赖，最长等待 5 分钟；安装完成后会复用缓存。</p>}
  </div>;
}
