import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { Check, ChevronDown, Search } from 'lucide-react';
import { t } from '../../lib/i18n';

export interface TaskPickerOption { id: string; label: string; detail?: string; group?: string; icon: ReactNode }

/** Keep popovers inside the modal's top layer so focus and pointer input remain available. */
export function TaskPicker({ label, value, options, onChange, disabled, searchPlaceholder, children, upwards = false }: {
  label: string; value: string; options: TaskPickerOption[]; onChange: (id: string) => void;
  disabled?: boolean; searchPlaceholder: string; children: ReactNode; upwards?: boolean;
}) {
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const filtered = options.filter(option => `${option.label} ${option.detail ?? ''}`.toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  useEffect(() => {
    if (!open) return;
    search.current?.focus();
    const outside = (event: Event) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('focusin', outside);
    return () => { document.removeEventListener('pointerdown', outside); document.removeEventListener('focusin', outside); };
  }, [open]);
  const close = () => { setOpen(false); trigger.current?.focus(); };
  return <div ref={root} className={`task-picker${upwards ? ' task-picker-upwards' : ''}`} onKeyDown={event => {
    if (!open) {
      if (event.key === 'ArrowDown' && !disabled) { event.preventDefault(); setQuery(''); setOpen(true); }
      return;
    }
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return; }
    if (event.key === 'Enter' && event.target === search.current) {
      event.preventDefault();
      if (filtered[0]) { onChange(filtered[0].id); close(); }
      return;
    }
    if (!['ArrowUp', 'ArrowDown'].includes(event.key)) return;
    const items = [...root.current?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? []];
    if (!items.length) return;
    event.preventDefault();
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = index < 0 ? (event.key === 'ArrowDown' ? 0 : items.length - 1) : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
    items[next].focus();
  }}>
    <button ref={trigger} type="button" className="task-context-button" aria-label={label} aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? id : undefined} disabled={disabled}
      onClick={() => { setQuery(''); setOpen(current => !current); }}>{children}<ChevronDown className="task-picker-chevron" /></button>
    {open && <div id={id} className="task-picker-popover" role="dialog" aria-label={label}>
      <div className="task-picker-search"><Search /><input ref={search} aria-label={searchPlaceholder} placeholder={searchPlaceholder} value={query} onChange={event => setQuery(event.target.value)} /></div>
      <div className="task-picker-options" role="listbox" aria-label={label}>
        {filtered.map((option, index) => <div key={option.id}>
          {option.group && option.group !== filtered[index - 1]?.group && <div className="task-picker-group">{option.group}</div>}
          <button type="button" role="option" aria-selected={value === option.id} onClick={() => { onChange(option.id); close(); }}>
            {option.icon}<span><strong>{option.label}</strong>{option.detail && <small>{option.detail}</small>}</span>{value === option.id && <Check className="task-picker-check" />}
          </button>
        </div>)}
        {!filtered.length && <p className="task-picker-empty">{t('tasks.no_matches')}</p>}
      </div>
    </div>}
  </div>;
}
