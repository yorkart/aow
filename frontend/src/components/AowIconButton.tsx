import type { ComponentPropsWithRef } from 'react';
import './aow-icon-button.css';

export function AowIconButton({ className = '', type = 'button', ...props }: ComponentPropsWithRef<'button'>) {
  return <button {...props} type={type} className={`aow-icon-button ${className}`} />;
}
