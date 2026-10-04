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
      <input aria-label="搜索需求" placeholder="搜索需求" value={query} onChange={event => onQuery(event.target.value)} />
      {data && <span className="inbox-match-count" aria-label={`显示 ${count} 条需求，共 ${data.items.length} 条`}>{count}{count !== data.items.length ? ` / ${data.items.length}` : ''}</span>}
    </label>
    <button ref={filter} className={`inbox-select inbox-filter-trigger${labels.length ? ' active' : ''}`} aria-label="按标签筛选" title="按标签筛选（匹配任一标签）"
      aria-haspopup="dialog" aria-expanded={open === 'filter'} aria-controls={open === 'filter' ? `${id}-filter` : undefined}
      disabled={!data} onClick={() => setOpen(value => value === 'filter' ? undefined : 'filter')}
      onKeyDown={event => { if (event.key === 'ArrowDown') { event.preventDefault(); setOpen('filter'); } }}>
      <Tag size={15} /><span className="inbox-filter-text">标签</span>{labels.length > 0 && <span className="inbox-filter-count">{labels.length}</span>}<ChevronDown size={12} className="inbox-filter-chevron" />
    </button>
    <button ref={manage} className={`inbox-label-menu-trigger${open === 'labels' ? ' active' : ''}`} aria-label="配置标签" title="配置标签"
      aria-haspopup="dialog" aria-expanded={open === 'labels'} aria-controls={open === 'labels' ? `${id}-labels` : undefined}
      disabled={!data} onClick={() => setOpen(value => value === 'labels' ? undefined : 'labels')}><MoreHorizontal size={18} /></button>
    {open === 'filter' && data && <RowMenu id={`${id}-filter`} className="inbox-filter-popover" role="dialog"
      label="按标签筛选（匹配任一标签）" anchor={filter} onClose={() => setOpen(undefined)}>
        <header><strong>标签过滤</strong><span>匹配任一标签</span></header>
        <div className="inbox-filter-list" role="group" aria-label="筛选标签">
          {data.labels.map(label => <label key={label.id} style={{ '--label-color': label.color } as CSSProperties}>
            <input type="checkbox" checked={labels.includes(label.id)} onChange={() => onLabel(label.id)} /><i /><span>{label.name}</span>
          </label>)}
          {!data.labels.length && <p>暂无标签，可在标签菜单中添加。</p>}
        </div>
        <footer><button disabled={!filtered} onClick={onClear}><X size={13} />清除筛选</button></footer>
    </RowMenu>}
    {open === 'labels' && data && <div ref={popup} id={`${id}-labels`} className="inbox-toolbar-popup inbox-labels-popover"
      role="dialog" aria-label="配置标签" onBlur={event => {
        const next = event.relatedTarget;
        if (next && !event.currentTarget.contains(next) && !filter.current?.contains(next) && !manage.current?.contains(next)) setOpen(undefined);
      }}>
      <LabelsEditor data={data} mutate={mutate} onClose={() => { setOpen(undefined); manage.current?.focus({ preventScroll: true }); }} />
    </div>}
  </header>;
}
