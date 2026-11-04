import type { ComponentPropsWithoutRef } from 'react';
import { useAowTooltip } from '../../components/AowTooltip';

export function ExplorerSearchButton({ label, onClick, ...props }: Omit<ComponentPropsWithoutRef<'button'>, 'title'> & { label: string }) {
  const { triggerProps, closeTooltip, tooltip } = useAowTooltip(label, 'bottom');
  return <>
    <button {...props} {...triggerProps} type="button" aria-label={label} onClick={event => { closeTooltip(); onClick?.(event); }} />
    {tooltip}
  </>;
}
