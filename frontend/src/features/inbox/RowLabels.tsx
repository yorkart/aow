import { useId, useRef, useState, type CSSProperties } from 'react';
import { ChevronDown } from 'lucide-react';
import type { InboxLabel } from './types';
import { RowMenu } from './RowMenu';

export function RowLabels({ labels, selected, busy, onChange }: {
  labels: InboxLabel[]; selected: string[]; busy: boolean; onChange: (ids: string[]) => void;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const active = labels.filter(label => selected.includes(label.id));
  return <>
    <button ref={trigger} className="inbox-select inbox-label-select" type="button" aria-label="选择标签" title={active.map(label => label.name).join('、') || '选择标签'}
      aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? menuId : undefined} disabled={busy}
      onClick={() => setOpen(value => !value)} onKeyDown={event => { if (event.key === 'ArrowDown') { event.preventDefault(); setOpen(true); } }}>
      <span className="inbox-tags">{active.length ? active.map(label => <span key={label.id} className="inbox-tag" style={{ color: label.color, borderColor: `${label.color}55` }}>{label.name}</span>) : <span>标签</span>}</span><ChevronDown size={12} />
    </button>
    {open && <RowMenu id={menuId} label="选择需求标签" role="dialog" anchor={trigger} onClose={() => setOpen(false)}>
      <fieldset aria-label="需求标签" disabled={busy}>
        {labels.map(label => <label key={label.id} style={{ '--label-color': label.color } as CSSProperties}><input type="checkbox" checked={selected.includes(label.id)} onChange={event => onChange(event.target.checked ? [...selected, label.id] : selected.filter(id => id !== label.id))} /><i /><span>{label.name}</span></label>)}
        {!labels.length && <p>暂无标签，可在面板顶部配置。</p>}
      </fieldset>
    </RowMenu>}
  </>;
}
