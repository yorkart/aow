import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check } from 'lucide-react';

export type TerminalSort = 'branch' | 'createdAt';
const sortOptions = [{ value: 'branch', label: '按分支名' }, { value: 'createdAt', label: '按创建时间' }] as const;

export function TerminalScopeMenu({ anchor, checked, sortBy, onSortChange, onChange, onClose, container }: {
  container?: HTMLElement;
  anchor: HTMLButtonElement;
  checked: boolean;
  sortBy: TerminalSort;
  onSortChange: (sortBy: TerminalSort) => void;
  onChange: (checked: boolean) => void;
  onClose: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  useLayoutEffect(() => {
    const trigger = anchor.getBoundingClientRect();
    const bounds = menu.current!.getBoundingClientRect();
    setPosition({
      x: Math.max(4, Math.min(trigger.right - bounds.width, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(trigger.bottom, window.innerHeight - bounds.height - 4)),
    });
    menu.current?.querySelector('button')?.focus();
  }, [anchor]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node) && !anchor.contains(event.target as Node)) onClose();
    };
    const close = () => onClose();
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); onClose(); anchor.focus(); }
      if (event.key === 'Tab') onClose();
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('keydown', key);
    };
  }, [anchor, onClose]);
  return createPortal(<div ref={menu} className="project-aow-context-menu terminal-scope-menu"
    style={{ left: position.x, top: position.y }} role="menu" aria-label="终端显示范围">
    <button role="menuitemcheckbox" aria-checked={checked} onClick={() => {
      onChange(!checked); onClose(); anchor.focus();
    }}><Check style={{ visibility: checked ? 'visible' : 'hidden' }} />显示所有Worktree</button>
    {checked ? <>
      <div className="terminal-scope-menu-separator" role="separator" />
      {sortOptions.map(option => <button key={option.value} role="menuitemradio" aria-checked={sortBy === option.value}
        onClick={() => { onSortChange(option.value); onClose(); anchor.focus(); }}>
        <Check style={{ visibility: sortBy === option.value ? 'visible' : 'hidden' }} />{option.label}
      </button>)}
    </> : null}
  </div>, container ?? document.body);
}
