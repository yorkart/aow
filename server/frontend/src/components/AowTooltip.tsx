import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type HTMLAttributes, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

type Placement = 'top' | 'bottom';

function AowTooltip({ id, anchor, children, onClose, placement }: {
  id: string; anchor: HTMLElement; children: ReactNode; onClose: () => void; placement: Placement;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  useLayoutEffect(() => {
    const bounds = ref.current?.getBoundingClientRect();
    if (!bounds) return;
    const rect = anchor.getBoundingClientRect();
    const above = rect.top - bounds.height - 4;
    const below = rect.bottom + 4;
    const top = placement === 'bottom'
      ? below + bounds.height <= window.innerHeight - 8 ? below : above
      : above >= 8 ? above : below;
    setPosition({
      left: Math.max(8, Math.min(rect.left, window.innerWidth - bounds.width - 8)),
      top: Math.max(8, Math.min(top, window.innerHeight - bounds.height - 8)),
    });
  }, [anchor, children, placement]);
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', keydown);
    window.addEventListener('scroll', onClose, true);
    window.addEventListener('resize', onClose);
    return () => {
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('scroll', onClose, true);
      window.removeEventListener('resize', onClose);
    };
  }, [onClose]);
  return createPortal(<div ref={ref} id={id} role="tooltip" className="aow-tooltip" style={position}>
    {children}
  </div>, document.body);
}

export function useAowTooltip(content: ReactNode, placement: Placement = 'top') {
  const id = useId();
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const closeTooltip = useCallback(() => setAnchor(null), []);
  const triggerProps: HTMLAttributes<HTMLElement> = content ? {
    'aria-describedby': anchor ? id : undefined,
    onPointerEnter: event => { if (event.pointerType !== 'touch') setAnchor(event.currentTarget); },
    onPointerLeave: closeTooltip,
    onPointerDown: closeTooltip,
    onFocus: event => { if (event.currentTarget.matches(':focus-visible')) setAnchor(event.currentTarget); },
    onBlur: closeTooltip,
  } : {};
  return {
    triggerProps,
    closeTooltip,
    tooltip: content && anchor ? <AowTooltip id={id} anchor={anchor} placement={placement} onClose={closeTooltip}>{content}</AowTooltip> : null,
  };
}
