import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, Copy, Info } from 'lucide-react';
import { copyText } from '../../lib/clipboard';
import './conversation-ids.css';

interface Identifier { label: string; value?: string | null }

function IdentifierRow({ label, value }: Identifier) {
  const [status, setStatus] = useState('');
  return <div className="conversation-id-row">
    <dt>{label}</dt><dd>{value || '暂无'}</dd>
    <button type="button" aria-label={`复制 ${label}`} title={status || `复制 ${label}`} disabled={!value} onClick={() => {
      if (!value) return;
      setStatus('');
      void copyText(value).then(() => setStatus('已复制')).catch(() => setStatus('复制失败，请选择文本后手动复制。'));
    }}>{status === '已复制' ? <Check size={14} /> : <Copy size={14} />}</button>
    {status && status !== '已复制' && <span className="conversation-id-status" role="alert">{status}</span>}
  </div>;
}

export function ConversationIds({ identifiers }: { identifiers: Identifier[] }) {
  const id = useId();
  const button = useRef<HTMLButtonElement>(null);
  const panel = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const cancelClose = useCallback(() => { clearTimeout(timer.current); timer.current = undefined; }, []);
  const close = useCallback(() => { cancelClose(); setOpen(false); }, [cancelClose]);
  const show = () => { cancelClose(); setOpen(true); };
  // Allow the pointer to cross the gap between the icon and the floating panel.
  const scheduleClose = useCallback(() => { cancelClose(); timer.current = setTimeout(close, 180); }, [cancelClose, close]);
  const contains = (target: EventTarget | null) => target instanceof Node && (button.current?.contains(target) || panel.current?.contains(target));

  useEffect(() => cancelClose, [cancelClose]);
  useLayoutEffect(() => {
    if (!open || !button.current || !panel.current) return;
    const update = () => {
      const anchor = button.current!.getBoundingClientRect();
      const size = panel.current!.getBoundingClientRect();
      setPosition({
        left: Math.max(8, Math.min(anchor.right - size.width, window.innerWidth - size.width - 8)),
        top: Math.max(8, Math.min(anchor.bottom + 6, window.innerHeight - size.height - 8)),
      });
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(panel.current);
    return () => observer.disconnect();
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!button.current?.contains(event.target as Node) && !panel.current?.contains(event.target as Node)) close();
    };
    // Replacing the hovered copy icon can suppress its ancestors' leave event.
    const move = (event: PointerEvent) => {
      if (event.pointerType === 'touch') return;
      if (button.current?.contains(event.target as Node) || panel.current?.contains(event.target as Node)) cancelClose();
      else if (timer.current === undefined) scheduleClose();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault(); event.stopPropagation();
      if (panel.current?.contains(document.activeElement)) button.current?.focus({ preventScroll: true });
      close();
    };
    const scroll = (event: Event) => { if (!panel.current?.contains(event.target as Node)) close(); };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('pointermove', move);
    document.addEventListener('keydown', escape, true);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', scroll, true);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('pointermove', move);
      document.removeEventListener('keydown', escape, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', scroll, true);
    };
  }, [open, close, cancelClose, scheduleClose]);

  return <>
    <button ref={button} type="button" className="conversation-ids-trigger" aria-label="会话 ID 信息" aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? id : undefined}
      onPointerEnter={event => { if (event.pointerType !== 'touch') show(); }} onPointerLeave={scheduleClose}
      onFocus={event => { if (event.currentTarget.matches(':focus-visible')) show(); }}
      onBlur={event => { if (!contains(event.relatedTarget)) scheduleClose(); }}
      onClick={show} onKeyDown={event => {
        if (event.key === 'Tab' && !event.shiftKey && open) {
          const first = panel.current?.querySelector<HTMLButtonElement>('button:not(:disabled)');
          if (first) { event.preventDefault(); first.focus(); }
        }
      }}><Info size={16} /></button>
    {open && createPortal(<div ref={panel} id={id} role="dialog" aria-label="会话 ID 信息" className="conversation-ids-panel aow-conversation-theme" style={position}
      onPointerEnter={cancelClose} onPointerLeave={scheduleClose} onFocus={cancelClose}
      onBlur={event => { if (!contains(event.relatedTarget)) scheduleClose(); }}
      onKeyDown={event => {
        if (event.key !== 'Tab') return;
        const buttons = panel.current?.querySelectorAll('button:not(:disabled)');
        if (buttons && event.target === buttons[event.shiftKey ? 0 : buttons.length - 1]) {
          if (event.shiftKey) event.preventDefault();
          button.current?.focus();
        }
      }}>
      <dl>{identifiers.map(identifier => <IdentifierRow key={`${identifier.label}:${identifier.value ?? ''}`} {...identifier} />)}</dl>
    </div>, document.body)}
  </>;
}
