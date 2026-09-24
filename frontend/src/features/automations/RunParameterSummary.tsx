import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import type { AutomationRun } from './types';

const SUMMARY_LIMIT = 100;

export function RunParameterSummary({ run }: { run: AutomationRun }) {
  const anchor = useRef<HTMLSpanElement>(null);
  const tooltip = useRef<HTMLDivElement>(null);
  const closeTimer = useRef<number | undefined>(undefined);
  const tooltipId = useId();
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const entries = Object.entries(run.variables ?? {});
  const summary = entries.map(([key, value]) => `${key}=${value}`).join(',').replace(/[\r\n\t]/g, ' ');
  const characters = Array.from(summary);
  const text = characters.length > SUMMARY_LIMIT ? `${characters.slice(0, SUMMARY_LIMIT).join('')}…` : summary;
  const keepOpen = () => window.clearTimeout(closeTimer.current);
  const closeSoon = () => { keepOpen(); closeTimer.current = window.setTimeout(() => setOpen(false), 160); };

  useEffect(() => () => window.clearTimeout(closeTimer.current), []);
  useLayoutEffect(() => {
    if (!open || !anchor.current || !tooltip.current) return;
    const bounds = tooltip.current.getBoundingClientRect();
    const rect = anchor.current.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(rect.left, window.innerWidth - bounds.width - 8)),
      top: rect.bottom + bounds.height + 6 <= window.innerHeight - 8
        ? rect.bottom + 6 : Math.max(8, rect.top - bounds.height - 6),
    });
  }, [open, run.variables]);
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    const scroll = (event: Event) => { if (!(event.target instanceof Node) || !tooltip.current?.contains(event.target)) close(); };
    window.addEventListener('keydown', keydown);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', scroll, true);
    return () => {
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', scroll, true);
    };
  }, [open]);

  if (!entries.length) return <span className="automation-parameter-summary automation-muted">{run.source === 'manual' && run.variables == null ? '未记录' : '—'}</span>;
  return <span ref={anchor} className="automation-parameter-summary" aria-describedby={open ? tooltipId : undefined}
    onPointerEnter={event => { if (event.pointerType !== 'touch') { keepOpen(); setOpen(true); } }} onPointerLeave={closeSoon}
    onClick={() => setOpen(false)}>
    <code>{text}</code>
    {open ? createPortal(<div ref={tooltip} id={tooltipId} role="tooltip" aria-label="执行参数" className="automation-parameter-tooltip" style={position}
      onPointerEnter={keepOpen} onPointerLeave={closeSoon} onPointerDown={event => event.stopPropagation()} onClick={event => event.stopPropagation()}>
      {entries.map(([key, value]) => <div className="automation-parameter-tooltip-row" key={key}><code>{key}</code><span>=</span><code>{value}</code></div>)}
    </div>, document.body) : null}
  </span>;
}
