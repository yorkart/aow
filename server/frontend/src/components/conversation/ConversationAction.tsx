import type { ButtonHTMLAttributes } from 'react';
import { useAowTooltip } from '../AowTooltip';

export function ConversationAction({ label, children, ...props }: Omit<ButtonHTMLAttributes<HTMLButtonElement>, 'title' | 'aria-label'> & { label: string }) {
  const { triggerProps, tooltip } = useAowTooltip(label, 'bottom');
  return <>
    <button type="button" {...props} {...triggerProps} aria-label={label}>{children}</button>
    {tooltip}
  </>;
}
