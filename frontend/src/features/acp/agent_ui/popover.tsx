// DOM adapter for the upstream implementation's anchored popovers. Business choices stay in each caller.
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { AowIconButton } from '../../../components/AowIconButton';

export function Popover({ label, trigger, children, disabled, above = false, iconButton = false }: {
  label: string; trigger: ReactNode; children: (close: () => void) => ReactNode; disabled?: boolean; above?: boolean; iconButton?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [style, setStyle] = useState<CSSProperties>({});
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const close = () => { setOpen(false); button.current?.focus(); };
  useLayoutEffect(() => {
    if (!open || !button.current || !menu.current) return;
    const anchor = button.current.getBoundingClientRect(); const size = menu.current.getBoundingClientRect();
    const computed = getComputedStyle(button.current);
    const colors = Object.fromEntries(['--aow-text', '--aow-muted', '--aow-panel', '--aow-line', '--aow-accent'].map(key => [key, computed.getPropertyValue(key)]));
    setStyle({ ...colors, left: Math.max(8, Math.min(anchor.right - size.width, innerWidth - size.width - 8)), top: Math.max(8, Math.min(above ? anchor.top - size.height - 6 : anchor.bottom + 6, innerHeight - size.height - 8)) });
    menu.current.querySelector<HTMLElement>('input, button:not(:disabled), [tabindex="0"]')?.focus();
  }, [open, above]);
  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => { if (!menu.current?.contains(event.target as Node) && !button.current?.contains(event.target as Node)) setOpen(false); };
    const resize = () => setOpen(false);
    document.addEventListener('pointerdown', outside); window.addEventListener('resize', resize);
    return () => { document.removeEventListener('pointerdown', outside); window.removeEventListener('resize', resize); };
  }, [open]);
  const Trigger = iconButton ? AowIconButton : 'button';
  return <><Trigger ref={button} type="button" className={iconButton ? undefined : 'acp-quiet-button'} aria-label={label} title={label} aria-haspopup="menu" aria-expanded={open} disabled={disabled} onClick={() => setOpen(value => !value)}>{trigger}</Trigger>
    {open && createPortal(<div ref={menu} className="acp-popover" role="menu" aria-label={label} style={style} onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); }
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        if (event.target instanceof HTMLInputElement && !['ArrowDown', 'ArrowUp'].includes(event.key)) return;
        const items = Array.from(menu.current?.querySelectorAll<HTMLElement>('button:not(:disabled)') ?? []);
        if (!items.length) return;
        event.preventDefault();
        const current = items.indexOf(document.activeElement as HTMLElement);
        const index = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (current + (event.key === 'ArrowUp' ? -1 : 1) + items.length) % items.length;
        items[index].focus();
      }
    }}>{children(close)}</div>, document.body)}
  </>;
}
