import { t } from '../../lib/i18n';
import { useEffect, useId, useRef, useState, type CSSProperties } from 'react';
import { ChevronDown, MoreHorizontal, Search, Tag, X } from 'lucide-react';
import type { AowProject } from '../../aow/types';
import { LabelsEditor } from './LabelsEditor';
import { ProjectSelect } from './ProjectSelect';
import { RowMenu } from './RowMenu';
import type { InboxSnapshot } from './types';

export function InboxToolbar({ visible, data, projects, project, query, labels, count, onProject, onQuery, onLabel, onClear, mutate }: {
  visible: boolean; data?: InboxSnapshot; projects: AowProject[]; project: string; query: string; labels: string[]; count: number;
  onProject: (id: string) => void; onQuery: (query: string) => void; onLabel: (id: string) => void; onClear: () => void;
  mutate: <T>(action: () => Promise<T>) => Promise<T>;
}) {
  const [open, setOpen] = useState<'filter' | 'labels'>();
  const id = useId();
  const popup = useRef<HTMLDivElement>(null);
  const filter = useRef<HTMLButtonElement>(null);
  const manage = useRef<HTMLButtonElement>(null);
  const filtered = !!project || !!query || labels.length > 0;

  useEffect(() => { if (!visible) setOpen(undefined); }, [visible]);
  useEffect(() => {
    if (open !== 'labels') return;
    const trigger = manage.current;
    popup.current?.querySelector<HTMLInputElement>('input:not([type=color])')?.focus({ preventScroll: true });
    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!popup.current?.contains(target) && !filter.current?.contains(target) && !manage.current?.contains(target)) setOpen(undefined);
    };
    const key = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault(); setOpen(undefined); trigger?.focus({ preventScroll: true });
      }
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('keydown', key);
    };
  }, [open]);

  return <header className="inbox-header">
    <ProjectSelect projects={projects} value={project} filter visible={visible} onChange={onProject} />
    <label className="inbox-search"><Search size={14} />
      <input aria-label={t("inbox.search_requirements")} placeholder={t("inbox.search_requirements")} value={query} onChange={event => onQuery(event.target.value)} />
      {data && <span className="inbox-match-count" aria-label={t("inbox.showing_of_requirements", { value0: count, value1: data.items.length })}>{count}{count !== data.items.length ? ` / ${data.items.length}` : ''}</span>}
    </label>
    <button ref={filter} className={`inbox-select inbox-filter-trigger${labels.length ? ' active' : ''}`} aria-label={t("inbox.filter_by_label")} title={t("inbox.filter_by_label_match_any")}
      aria-haspopup="dialog" aria-expanded={open === 'filter'} aria-controls={open === 'filter' ? `${id}-filter` : undefined}
      disabled={!data} onClick={() => setOpen(value => value === 'filter' ? undefined : 'filter')}
      onKeyDown={event => { if (event.key === 'ArrowDown') { event.preventDefault(); setOpen('filter'); } }}>
      <Tag size={15} /><span className="inbox-filter-text">{t("inbox.labels")}</span>{labels.length > 0 && <span className="inbox-filter-count">{labels.length}</span>}<ChevronDown size={12} className="inbox-filter-chevron" />
    </button>
    <button ref={manage} className={`inbox-label-menu-trigger${open === 'labels' ? ' active' : ''}`} aria-label={t("inbox.configure_labels")} title={t("inbox.configure_labels")}
      aria-haspopup="dialog" aria-expanded={open === 'labels'} aria-controls={open === 'labels' ? `${id}-labels` : undefined}
      disabled={!data} onClick={() => setOpen(value => value === 'labels' ? undefined : 'labels')}><MoreHorizontal size={18} /></button>
    {open === 'filter' && data && <RowMenu id={`${id}-filter`} className="inbox-filter-popover" role="dialog"
      label={t("inbox.filter_by_label_match_any")} anchor={filter} onClose={() => setOpen(undefined)}>
        <header><strong>{t("inbox.label_filter")}</strong><span>{t("inbox.match_any_label")}</span></header>
        <div className="inbox-filter-list" role="group" aria-label={t("inbox.filter_labels")}>
          {data.labels.map(label => <label key={label.id} style={{ '--label-color': label.color } as CSSProperties}>
            <input type="checkbox" checked={labels.includes(label.id)} onChange={() => onLabel(label.id)} /><i /><span>{label.name}</span>
          </label>)}
          {!data.labels.length && <p>{t("inbox.no_labels_add_some_in_the_labels_menu")}</p>}
        </div>
        <footer><button disabled={!filtered} onClick={onClear}><X size={13} />{t("inbox.clear_filters")}</button></footer>
    </RowMenu>}
    {open === 'labels' && data && <div ref={popup} id={`${id}-labels`} className="inbox-toolbar-popup inbox-labels-popover"
      role="dialog" aria-label={t("inbox.configure_labels")} onBlur={event => {
        const next = event.relatedTarget;
        if (next && !event.currentTarget.contains(next) && !filter.current?.contains(next) && !manage.current?.contains(next)) setOpen(undefined);
      }}>
      <LabelsEditor data={data} mutate={mutate} onClose={() => { setOpen(undefined); manage.current?.focus({ preventScroll: true }); }} />
    </div>}
  </header>;
}
