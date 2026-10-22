import { useEffect, useId, useRef, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
import { RowMenu } from './RowMenu';
import './aow-select.css';

export function AowSelect({ id, label, value, options, onChange, disabled = false }: {
  id?: string; label: string; value: string; options: readonly { value: string; label: string }[];
  onChange: (value: string) => void; disabled?: boolean;
}) {
  const generatedId = useId();
  const controlId = id ?? generatedId;
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const selected = options.find(option => option.value === value);
  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);
  return <>
    <button ref={trigger} id={controlId} type="button" className="project-aow-dialog-button aow-select"
      aria-label={label} aria-describedby={`${controlId}-value`} aria-haspopup="listbox" aria-expanded={open && !disabled}
      aria-controls={open && !disabled ? `${controlId}-menu` : undefined} disabled={disabled}
      onClick={() => setOpen(current => !current)}
      onKeyDown={event => { if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setOpen(true); } }}>
      <span className="aow-select-value" id={`${controlId}-value`}>{selected?.label}</span><ChevronDown size={14} aria-hidden="true" />
    </button>
    {open && !disabled && <RowMenu id={`${controlId}-menu`} label={label} role="listbox" anchor={trigger} onClose={() => setOpen(false)}>
      {options.map(option => <button key={option.value} type="button" role="option" aria-selected={option.value === value} tabIndex={-1}
        onClick={() => { setOpen(false); trigger.current?.focus({ preventScroll: true }); if (value !== option.value) onChange(option.value); }}>
        <span>{option.label}</span>{option.value === value && <Check size={14} aria-hidden="true" />}
      </button>)}
    </RowMenu>}
  </>;
}
