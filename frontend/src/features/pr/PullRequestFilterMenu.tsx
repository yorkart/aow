import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Check, MoreHorizontal } from 'lucide-react';
import { AowIconButton } from '../../components/AowIconButton';

export function PullRequestFilterMenu({ hideDrafts, onChange }: {
  hideDrafts: boolean;
  onChange: (hideDrafts: boolean) => void;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const close = useCallback((restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) trigger.current?.focus();
  }, []);

  useLayoutEffect(() => {
    if (!open || !trigger.current || !menu.current) return;
    const anchor = trigger.current.getBoundingClientRect();
    const bounds = menu.current.getBoundingClientRect();
    setPosition({
      left: Math.max(4, Math.min(anchor.right - bounds.width, window.innerWidth - bounds.width - 4)),
      top: Math.max(4, Math.min(anchor.bottom + 4, window.innerHeight - bounds.height - 4)),
    });
    menu.current.querySelector('button')?.focus();
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const dismiss = () => close();
    const outside = (event: PointerEvent) => {
      if (!trigger.current?.contains(event.target as Node) && !menu.current?.contains(event.target as Node)) close();
    };
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); close(true); }
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('scroll', dismiss, true);
    window.addEventListener('resize', dismiss);
    window.addEventListener('keydown', keydown);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('scroll', dismiss, true);
      window.removeEventListener('resize', dismiss);
      window.removeEventListener('keydown', keydown);
    };
  }, [open, close]);

  return <>
    <AowIconButton ref={trigger} title="Open PRs 过滤选项" aria-label="Open PRs 过滤选项" aria-haspopup="menu"
      aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(value => !value)}>
      <MoreHorizontal />
    </AowIconButton>
    {open ? createPortal(<div ref={menu} id={id} className="project-aow-context-menu pull-requests-filter-menu"
      role="menu" aria-label="Open PRs 过滤选项" style={position}
      onPointerDown={event => event.stopPropagation()}
      onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget) && !trigger.current?.contains(event.relatedTarget)) close(); }}>
      <button type="button" role="menuitemcheckbox" aria-checked={hideDrafts} onClick={() => { onChange(!hideDrafts); close(true); }}>
        <span>过滤 Draft</span>{hideDrafts ? <Check /> : null}
      </button>
    </div>, document.body) : null}
  </>;
}
