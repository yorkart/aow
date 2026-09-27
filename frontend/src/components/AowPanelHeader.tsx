import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type HTMLAttributes, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { ChevronRight } from 'lucide-react';
import { AowIconButton } from './AowIconButton';

export interface AowPanelHeaderProps {
  title: string;
  tooltip?: ReactNode;
  icon?: ReactNode;
  className?: string;
  collapsed?: boolean;
  controlsId?: string;
  onToggle?: () => void;
  onNavigate?: () => void;
  details?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}

function PanelHeaderTooltip({ id, anchor, children, onClose }: {
  id: string; anchor: HTMLElement; children: ReactNode; onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  useLayoutEffect(() => {
    const bounds = ref.current?.getBoundingClientRect();
    if (!bounds) return;
    const rect = anchor.getBoundingClientRect();
    setPosition({
      left: Math.max(8, Math.min(rect.left, window.innerWidth - bounds.width - 8)),
      top: Math.max(0, rect.top - bounds.height - 4),
    });
  }, [anchor, children]);
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
  return createPortal(<div ref={ref} id={id} role="tooltip" className="aow-panel-header-tooltip" style={position}>
    {children}
  </div>, document.body);
}

export function AowPanelHeader({ title, tooltip, icon, className, collapsed = false, controlsId, onToggle, onNavigate, details, actions, children }: AowPanelHeaderProps) {
  const tooltipId = useId();
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const closeTooltip = useCallback(() => setAnchor(null), []);
  const tooltipProps: HTMLAttributes<HTMLElement> = tooltip ? {
    'aria-describedby': anchor ? tooltipId : undefined,
    onPointerEnter: event => { if (event.pointerType !== 'touch') setAnchor(event.currentTarget); },
    onPointerLeave: closeTooltip,
    onPointerDown: closeTooltip,
    onFocus: event => { if (event.currentTarget.matches(':focus-visible')) setAnchor(event.currentTarget); },
    onBlur: closeTooltip,
  } : {};
  const content = <>{onToggle && !onNavigate ? <ChevronRight className={collapsed ? undefined : 'expanded'} /> : null}{icon}<strong>{title}</strong>{details}</>;

  return <header className={`side-title aow-panel-header${className ? ` ${className}` : ''}`}>
    {onNavigate && onToggle ? <AowIconButton className="aow-panel-toggle" aria-label={`${collapsed ? '展开' : '收起'} ${title}`} aria-expanded={!collapsed} aria-controls={controlsId}
      onClick={() => { closeTooltip(); onToggle(); }}><ChevronRight className={collapsed ? undefined : 'expanded'} /></AowIconButton> : null}
    {onNavigate
      ? <button className="aow-panel-header-title" type="button" aria-label={`定位 ${title}`} {...tooltipProps} onClick={() => { closeTooltip(); onNavigate(); }}>{content}</button>
      : onToggle
      ? <button className="aow-panel-header-title" type="button" aria-expanded={!collapsed} aria-controls={controlsId}
        {...tooltipProps} onClick={() => { closeTooltip(); onToggle(); }}>{content}</button>
      : <span className="aow-panel-header-title" {...tooltipProps} tabIndex={tooltip ? 0 : undefined}>{content}</span>}
    {actions ? <span className="aow-panel-header-actions">{actions}</span> : null}
    {children}
    {tooltip && anchor ? <PanelHeaderTooltip id={tooltipId} anchor={anchor} onClose={closeTooltip}>{tooltip}</PanelHeaderTooltip> : null}
  </header>;
}
