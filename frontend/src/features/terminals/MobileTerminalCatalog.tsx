import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { List, X } from 'lucide-react';

export function MobileTerminalCatalog({ id, onClose, returnFocus, children }: {
  id: string;
  onClose: () => void;
  returnFocus: HTMLElement | null;
  children: (container: HTMLDialogElement) => ReactNode;
}) {
  const [dialog, setDialog] = useState<HTMLDialogElement | null>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const backdropPointer = useRef(false);
  useLayoutEffect(() => {
    if (!dialog) return;
    dialog.showModal();
    title.current?.focus({ preventScroll: true });
    return () => { dialog.close(); returnFocus?.focus({ preventScroll: true }); };
  }, [dialog, returnFocus]);
  const outside = (event: React.PointerEvent<HTMLDialogElement> | React.MouseEvent<HTMLDialogElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    return event.target === event.currentTarget && (event.clientX < bounds.left || event.clientX > bounds.right
      || event.clientY < bounds.top || event.clientY > bounds.bottom);
  };
  return <dialog ref={setDialog} id={id} className="mobile-terminal-catalog" aria-labelledby={`${id}-title`}
    onCancel={event => { event.preventDefault(); onClose(); }}
    onPointerDown={event => { backdropPointer.current = outside(event); }}
    onClick={event => {
      if (backdropPointer.current && outside(event)) onClose();
      backdropPointer.current = false;
    }}
    onKeyDown={event => {
      // Let the menu handle Escape before dismissing the whole catalog.
      if (event.key === 'Escape' && event.currentTarget.querySelector('[role="menu"]')) event.preventDefault();
    }}>
    <header className="mobile-terminal-catalog-header">
      <List size={20} /><h2 ref={title} id={`${id}-title`} tabIndex={-1}>终端列表</h2>
      <button className="mobile-icon-button" aria-label="关闭终端列表" onClick={onClose}><X size={21} /></button>
    </header>
    {dialog && children(dialog)}
  </dialog>;
}
