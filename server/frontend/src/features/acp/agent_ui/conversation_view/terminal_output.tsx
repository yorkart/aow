import { t } from '../../../../lib/i18n';
import { useEffect, useRef } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { object, text, type Data } from '../../types';

export function TerminalOutput({ terminal, pending }: { terminal?: Data; pending: boolean }) {
  const container = useRef<HTMLDivElement>(null);
  const display = useRef<Terminal | null>(null);
  const written = useRef('');
  const fitOutput = useRef<() => void>(() => undefined);
  const output = text(terminal?.output);
  useEffect(() => {
    if (!container.current) return;
    const element = container.current;
    const style = getComputedStyle(element);
    const fontSize = parseFloat(style.fontSize);
    const lineHeight = parseFloat(style.lineHeight) / fontSize;
    const view = new Terminal({ disableStdin: true, convertEol: true, cursorBlink: false, cursorInactiveStyle: 'none', rows: 1, fontSize, fontFamily: style.fontFamily, lineHeight, scrollback: 10000, theme: { background: style.backgroundColor, foreground: style.color } });
    const fit = new FitAddon();
    view.loadAddon(fit);
    view.open(container.current);
    display.current = view;
    written.current = '';
    fitOutput.current = () => {
      if (!element.clientWidth) return;
      const style = getComputedStyle(element);
      const fontSize = parseFloat(style.fontSize);
      if (view.options.fontSize !== fontSize) view.options.fontSize = fontSize;
      const buffer = view.buffer.active;
      // Keep the trailing cursor row: omitting it hides the last output line
      // when Xterm has already scrolled a newline into its history buffer.
      const rows = Math.max(1, buffer.baseY + buffer.cursorY + 1);
      element.style.height = `${Math.min(288, Math.ceil(rows * parseFloat(style.lineHeight)) + 12)}px`;
      fit.fit();
    };
    const resize = new ResizeObserver(() => fitOutput.current());
    resize.observe(element);
    return () => { resize.disconnect(); fitOutput.current = () => undefined; display.current = null; view.dispose(); };
  }, []);
  useEffect(() => {
    const view = display.current;
    if (!view) return;
    if (!output.startsWith(written.current)) { view.reset(); written.current = ''; }
    const chunk = output.slice(written.current.length);
    if (chunk) view.write(chunk, () => { if (display.current === view) fitOutput.current(); });
    written.current = output;
  }, [output]);
  const exit = object(terminal?.exit_status);
  return <div className="acp-terminal-output" data-exit-code={typeof exit.exitCode === 'number' ? exit.exitCode : undefined}>
    <div ref={container} className="acp-terminal-screen acp-code" aria-label={t("acp.command_output")} hidden={!output} />
    {!output && <p className="acp-muted">{pending ? t("acp.waiting_for_command_output") : terminal ? t("acp.the_command_produced_no_output") : t("acp.the_agent_did_not_provide_terminal_output")}</p>}
  </div>;
}
