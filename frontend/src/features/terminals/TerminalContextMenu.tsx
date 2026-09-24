import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { RefreshCw, Trash2 } from 'lucide-react';
import { WorkspaceMenuLabel } from '../../aow/WorkspaceMenuLabel';

export function TerminalContextMenu({ x, y, title, onClose, onOpen, onOpenFloating, onRebuild, rebuilding, onDestroy, container }: {
  container?: HTMLElement;
  x: number;
  y: number;
  title: string;
  onClose: () => void;
  onOpen: () => void;
  onOpenFloating?: () => void;
  onRebuild?: () => void;
  rebuilding?: boolean;
  onDestroy: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)),
    });
  }, [x, y]);

  useEffect(() => {
    const close = () => onClose();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', key);
    };
  }, [onClose]);

  const action = (callback: () => void) => () => { onClose(); callback(); };
  return createPortal(<div ref={menu} className="project-aow-context-menu project-aow-terminal-menu"
    style={{ left: position.x, top: position.y }} role="menu" aria-label={`${title} 终端操作`}
    onPointerDown={event => event.stopPropagation()} onContextMenu={event => event.preventDefault()}>
    <button role="menuitem" onClick={action(onOpen)}><WorkspaceMenuLabel action="open" /></button>
    {onOpenFloating ? <button role="menuitem" onClick={action(onOpenFloating)}><WorkspaceMenuLabel action="floating" /></button> : null}
    {onRebuild ? <button role="menuitem" disabled={rebuilding} onClick={action(onRebuild)}><RefreshCw />{rebuilding ? '重建中…' : '重建'}</button> : null}
    <button className="danger" role="menuitem" onClick={action(onDestroy)}><Trash2 />销毁</button>
  </div>, container ?? document.body);
}
