import { useRef, type ReactNode } from 'react';
import { MoreHorizontal } from 'lucide-react';
import { useListRowMenu } from './ListRowMenuContext';
import { AowIconButton } from './AowIconButton';
import './aow-list-row.css';

export function AowListRow({ className, openClassName = '', icon, title, tooltip, children, role, menuLabel,
  menuExpanded, onMenu, onOpen, actions, titlePrefix, openLabel, active, disabled }: {
  className: string;
  openClassName?: string;
  icon?: ReactNode;
  title: string;
  titlePrefix?: ReactNode;
  openLabel?: string;
  active?: boolean;
  disabled?: boolean;
  actions?: ReactNode;
  tooltip?: string;
  children: ReactNode;
  role?: 'listitem';
  menuLabel: string;
  menuExpanded?: boolean;
  onMenu?: (x: number, y: number) => void;
  onOpen: () => void;
}) {
  const openButton = useRef<HTMLButtonElement>(null);
  const workspaceMenu = useListRowMenu();
  const openMenu = (x: number, y: number) => {
    if (onMenu) onMenu(x, y);
    else if (openButton.current) workspaceMenu?.open(openButton.current, x, y);
  };

  return <div className={`aow-list-row ${className}`} role={role} onContextMenu={event => {
    if (actions || disabled) return;
    event.preventDefault(); event.stopPropagation();
    const bounds = event.currentTarget.getBoundingClientRect();
    openMenu(event.clientX || bounds.x, event.clientY || bounds.bottom);
  }}>
    <button ref={openButton} type="button" data-workspace-open={onMenu || actions ? undefined : true}
      className={`aow-list-row-open ${openClassName}`} title={tooltip} aria-label={openLabel} aria-current={active ? 'true' : undefined} disabled={disabled} onClick={onOpen}>
      {icon}
      <span className="aow-list-row-content">
        <strong className="aow-list-row-title">{titlePrefix}{title}</strong>
        {children}
      </span>
    </button>
    {actions ?? <AowIconButton disabled={disabled} className="aow-list-row-menu" title={menuLabel} aria-label={`${title} ${menuLabel}`}
      aria-haspopup="menu" aria-expanded={menuExpanded ?? (workspaceMenu?.target === openButton.current)}
      onClick={event => {
        event.stopPropagation();
        const bounds = event.currentTarget.getBoundingClientRect();
        openMenu(bounds.right, bounds.bottom);
      }}><MoreHorizontal /></AowIconButton>}
  </div>;
}
