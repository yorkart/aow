// Display-only terminal output from the adapter; no process or input channel.
import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { object, text, type Data } from '../../types';

export function TerminalOutput({ terminal, pending }: { terminal?: Data; pending: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const display = useRef<Terminal | null>(null);
  const written = useRef('');
  const output = text(terminal?.output);
  useEffect(() => {
    if (!container.current) return;
    const view = new Terminal({ disableStdin: true, convertEol: true, cursorBlink: false, fontSize: 12, scrollback: 10000, theme: { background: '#1e1e1e', foreground: '#d3d7df' } });
    const fit = new FitAddon();
    view.loadAddon(fit);
    view.open(container.current);
    display.current = view;
    written.current = '';
    const resize = new ResizeObserver(() => { if (container.current?.clientWidth) fit.fit(); });
    resize.observe(container.current);
    return () => { resize.disconnect(); display.current = null; view.dispose(); };
  }, []);
  useEffect(() => {
    const view = display.current;
    if (!view) return;
    if (!output.startsWith(written.current)) { view.reset(); written.current = ''; }
    const chunk = output.slice(written.current.length);
    if (chunk) view.write(chunk);
    written.current = output;
  }, [output]);
  const exit = object(terminal?.exit_status);
  return <div className="zed-terminal-output">
    {typeof terminal?.cwd === 'string' && <div className="zed-muted">{terminal.cwd}</div>}
    <div ref={container} className="zed-terminal-screen" aria-label="命令输出" hidden={!output} />
    {!output && <p className="zed-muted">{pending ? '等待命令输出…' : terminal ? '命令未产生输出' : 'Agent 未提供终端输出'}</p>}
    {terminal?.exit_status != null && <div className="zed-muted">{typeof exit.exitCode === 'number' ? `退出码：${exit.exitCode}` : '命令已结束'}{exit.signal ? ` · ${text(exit.signal)}` : ''}</div>}
  </div>;
}
