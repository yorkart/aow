import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

export function InboxMenu({ x, y, anchor, label, children, onClose }: {
  x: number;
  y: number;
  anchor?: HTMLButtonElement;
  label: string;
  children: ReactNode;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });
  useLayoutEffect(() => {
    const bounds = menu.current!.getBoundingClientRect();
    setPosition({
      x: Math.max(4, Math.min(anchor ? x - bounds.width : x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)),
    });
    menu.current?.querySelector('button')?.focus();
  }, [x, y, anchor]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node) && !anchor?.contains(event.target as Node)) onClose();
    };
    const scroll = (event: Event) => {
      if (!(event.target instanceof Node) || !menu.current?.contains(event.target)) onClose();
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); onClose(); anchor?.focus(); }
      if (event.key === 'Tab') onClose();
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault();
        const buttons = [...menu.current!.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1
          : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('scroll', scroll, true);
    window.addEventListener('resize', onClose);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('scroll', scroll, true);
      window.removeEventListener('resize', onClose);
      window.removeEventListener('keydown', key);
    };
  }, [anchor, onClose]);
  return createPortal(<div ref={menu} className="project-aow-context-menu" role="menu" aria-label={label}
    style={{ left: position.x, top: position.y, maxHeight: 'calc(100dvh - 8px)', overflowY: 'auto' }}
    onContextMenu={event => event.preventDefault()}>{children}</div>, document.body);
}
