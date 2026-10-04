import { useEffect, useLayoutEffect, useRef, useState, type ReactNode, type RefObject } from 'react';
import { createPortal } from 'react-dom';

export function RowMenu({ id, label, role = 'menu', anchor, onClose, children, className = '' }: {
  id: string; label: string; role?: 'menu' | 'dialog' | 'listbox'; anchor: RefObject<HTMLButtonElement | null>; onClose: () => void; children: ReactNode; className?: string;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  const [position, setPosition] = useState({ top: 0, left: 0, width: 240, maxHeight: 240 });
  useLayoutEffect(() => {
    const panel = anchor.current?.closest('.inbox-panel');
    const update = () => {
      const bounds = anchor.current?.getBoundingClientRect();
      if (!bounds || !menu.current) return;
      const panelBounds = panel?.getBoundingClientRect();
      const left = Math.max(8, (panelBounds?.left ?? 0) + 8);
      const right = Math.min(window.innerWidth - 8, (panelBounds?.right ?? window.innerWidth) - 8);
      const top = Math.max(8, (panelBounds?.top ?? 0) + 8);
      const bottom = Math.min(window.innerHeight - 8, (panelBounds?.bottom ?? window.innerHeight) - 8);
      const width = Math.max(0, Math.min(240, right - left));
      const below = bottom - bounds.bottom - 4;
      const above = bounds.top - top - 4;
      const desired = Math.min(240, menu.current.scrollHeight + 2);
      const upwards = below < desired && above > below;
      const maxHeight = Math.max(32, Math.min(240, upwards ? above : below));
      const height = Math.min(desired, maxHeight);
      setPosition({ top: upwards ? bounds.top - height - 4 : bounds.bottom + 4, left: Math.max(left, Math.min(bounds.left, right - width)), width, maxHeight });
    };
    update();
    const observer = new ResizeObserver(update);
    if (menu.current) observer.observe(menu.current);
    if (anchor.current) observer.observe(anchor.current);
    if (panel) observer.observe(panel);
    window.addEventListener('resize', update);
    window.addEventListener('scroll', update, true);
    return () => { observer.disconnect(); window.removeEventListener('resize', update); window.removeEventListener('scroll', update, true); };
  }, [anchor]);
  useEffect(() => {
    const outside = (event: Event) => { if (!anchor.current?.contains(event.target as Node) && !menu.current?.contains(event.target as Node)) close.current(); };
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.preventDefault(); close.current(); anchor.current?.focus(); } };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('focusin', outside);
    document.addEventListener('keydown', escape);
    (menu.current?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]') ?? menu.current?.querySelector<HTMLElement>('input:not(:disabled), button:not(:disabled)') ?? menu.current)?.focus({ preventScroll: true });
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('focusin', outside); document.removeEventListener('keydown', escape); };
  }, [anchor]);
  return createPortal(<div ref={menu} id={id} className={`inbox-row-menu ${className}`} data-inbox-editor-owner={anchor.current?.closest('[data-inbox-editor]')?.getAttribute('data-inbox-editor')}
    data-floating-workspace={anchor.current?.closest('[data-floating-workspace]') ? '' : undefined} role={role} aria-label={label} tabIndex={-1} style={position} onKeyDown={event => {
    if (role === 'dialog' || !['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return;
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled), [role="option"]:not(:disabled)') ?? [])];
    if (!items.length) return;
    event.preventDefault();
    const current = items.indexOf(document.activeElement as HTMLButtonElement);
    const index = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (current + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[index].focus();
  }}>{children}</div>, document.body);
}
