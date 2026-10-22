import { t } from '../../lib/i18n';
import { useEffect, useId, useRef, useState } from 'react';
import { Check, ChevronDown } from 'lucide-react';
import type { AowProject } from '../../aow/types';
import { RowMenu } from './RowMenu';

export function ProjectSelect({ projects, value, onChange, disabled = false, filter = false, visible = true }: {
  projects: AowProject[]; value: string; onChange: (id: string) => void; disabled?: boolean; filter?: boolean; visible?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const label = filter ? t("inbox.filter_by_project") : t("inbox.bind_project");
  const options = [
    ...(filter ? [{ id: '', name: t("inbox.all_projects") }, { id: 'unbound', name: t("inbox.no_project_bound") }] : [{ id: '', name: t("inbox.no_project_bound") }]),
    ...projects,
  ];
  if (!options.some(option => option.id === value)) options.push({ id: value, name: t("inbox.project_removed") });
  const selected = options.find(option => option.id === value)!;
  useEffect(() => { if (disabled || !visible) setOpen(false); }, [disabled, visible]);
  return <>
    <button ref={trigger} type="button" className={`inbox-select ${filter ? `inbox-project-filter${value ? ' active' : ''}` : 'inbox-project-select'}`}
      aria-label={label} title={selected.name} aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? menuId : undefined}
      disabled={disabled} onClick={() => setOpen(current => !current)}
      onKeyDown={event => { if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setOpen(true); } }}>
      <span className="inbox-select-text">{selected.name}</span><ChevronDown size={12} />
    </button>
    {open && <RowMenu id={menuId} label={label} role="listbox" anchor={trigger} onClose={() => setOpen(false)}>
      {options.map(option => <button key={option.id} type="button" role="option" aria-selected={option.id === value} tabIndex={-1}
        onClick={() => { setOpen(false); trigger.current?.focus({ preventScroll: true }); if (value !== option.id) onChange(option.id); }}>
        <span>{option.name}</span>{option.id === value && <Check size={14} className="inbox-option-check" />}
      </button>)}
    </RowMenu>}
  </>;
}
