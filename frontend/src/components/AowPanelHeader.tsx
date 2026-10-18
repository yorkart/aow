import type { ReactNode } from 'react';
import { ChevronRight } from 'lucide-react';
import { AowIconButton } from './AowIconButton';
import { useAowTooltip } from './AowTooltip';

export interface AowPanelHeaderProps {
  title: string;
  tooltip?: ReactNode;
  icon?: ReactNode;
  className?: string;
  collapsed?: boolean;
  controlsId?: string;
  onToggle?: () => void;
  onNavigate?: () => void;
  togglePosition?: 'start' | 'end';
  details?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}

export function AowPanelHeader({ title, tooltip, icon, className, collapsed = false, controlsId, onToggle, onNavigate, togglePosition = 'start', details, actions, children }: AowPanelHeaderProps) {
  const { triggerProps: tooltipProps, closeTooltip, tooltip: tooltipPopup } = useAowTooltip(tooltip);
  const content = <>{onToggle && !onNavigate && togglePosition === 'start' ? <ChevronRight className={collapsed ? undefined : 'expanded'} /> : null}{icon}<strong>{title}</strong>{details}</>;
  const toggle = onToggle && (onNavigate || togglePosition === 'end') ? <AowIconButton className="aow-panel-toggle" aria-label={`${collapsed ? '展开' : '收起'} ${title}`} aria-expanded={!collapsed} aria-controls={controlsId}
    onClick={() => { closeTooltip(); onToggle(); }}><ChevronRight className={collapsed ? undefined : 'expanded'} /></AowIconButton> : null;

  return <header className={`side-title aow-panel-header${className ? ` ${className}` : ''}`} onClick={togglePosition === 'end' && onToggle ? event => {
    if (event.defaultPrevented || (event.target instanceof Element && event.target.closest('button, a, input, select, textarea, [role="button"]'))) return;
    closeTooltip();
    (onNavigate ?? onToggle)();
  } : undefined}>
    {togglePosition === 'start' ? toggle : null}
    {onNavigate
      ? <button className="aow-panel-header-title" type="button" aria-label={`定位 ${title}`} {...tooltipProps} onClick={() => { closeTooltip(); onNavigate(); }}>{content}</button>
      : onToggle
      ? <button className="aow-panel-header-title" type="button" aria-label={togglePosition === 'end' ? title : undefined} aria-expanded={!collapsed} aria-controls={controlsId}
        {...tooltipProps} onClick={() => { closeTooltip(); onToggle(); }}>{content}</button>
      : <span className="aow-panel-header-title" {...tooltipProps} tabIndex={tooltip ? 0 : undefined}>{content}</span>}
    {actions || (togglePosition === 'end' && toggle) ? <span className="aow-panel-header-actions">{togglePosition === 'end' ? toggle : null}{actions}</span> : null}
    {children}
    {tooltipPopup}
  </header>;
}
