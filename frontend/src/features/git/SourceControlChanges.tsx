import { memo, useCallback, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent } from 'react';
import { ChevronRight } from 'lucide-react';
import { DirectoryTypeIcon, FileTypeIcon } from '../files/FileTypeIcon';

export interface ChangeItem { id: string; path: string; status: string; title: string }
interface Directory { name: string; path: string; directories: Directory[]; files: ChangeItem[] }
type Row = { type: 'directory'; directory: Directory; depth: number } | { type: 'file'; item: ChangeItem; depth: number };
interface Props {
  items: ChangeItem[];
  layout: 'list' | 'tree';
  active: boolean;
  selectedId?: string;
  onOpen: (item: ChangeItem) => void;
}

const rowHeight = 24;
const overscan = 8;
const layoutEvent = 'source-control-list-layout';

function buildTree(items: ChangeItem[]): Directory {
  const root: Directory = { name: '', path: '', directories: [], files: [] };
  const directories = new Map<string, Directory>([['', root]]);
  for (const item of items) {
    const segments = item.path.split('/');
    segments.pop();
    let parent = root;
    for (const name of segments) {
      const path = parent.path ? `${parent.path}/${name}` : name;
      let directory = directories.get(path);
      if (!directory) {
        directory = { name, path, directories: [], files: [] };
        directories.set(path, directory);
        parent.directories.push(directory);
      }
      parent = directory;
    }
    parent.files.push(item);
  }
  for (const directory of directories.values()) {
    directory.directories.sort((a, b) => a.name.localeCompare(b.name));
    directory.files.sort((a, b) => a.path.localeCompare(b.path));
  }
  return root;
}

const FileRow = memo(function FileRow({ item, compact, depth, selected, onOpen }: {
  item: ChangeItem; compact: boolean; depth: number; selected: boolean; onOpen: Props['onOpen'];
}) {
  const slash = item.path.lastIndexOf('/');
  return <button data-workspace-open className={`tree-row change-row${selected ? ' selected' : ''}`} title={item.title}
    style={{ '--tree-depth': depth } as CSSProperties}
    aria-current={selected || undefined} onClick={() => onOpen(item)}>
    <span className="chevron" aria-hidden="true" />
    <FileTypeIcon path={item.path} className="change-file-icon" />
    <span className="change-name">{compact ? item.path.slice(slash + 1) : item.path}</span>
    {!compact && slash >= 0 ? <span className="change-directory">{item.path.slice(0, slash)}</span> : null}
    <b>{item.status}</b>
  </button>;
});

// Panel lists share the sidebar viewport; standalone lists use their local
// Changes/Commits viewport. Expanded commit files never add a nested scrollbar.
export const SourceControlChanges = memo(function SourceControlChanges({ items, layout, active, selectedId, onOpen }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const viewport = useRef<HTMLElement | null>(null);
  const pendingFocus = useRef<number | undefined>(undefined);
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [range, setRange] = useState({ start: 0, end: 0 });
  const tree = useMemo(() => layout === 'tree' ? buildTree(items) : undefined, [items, layout]);
  const rows = useMemo(() => {
    if (!tree) return items.map<Row>(item => ({ type: 'file', item, depth: 0 }));
    const result: Row[] = [];
    const visit = (node: Directory, depth: number) => {
      for (const directory of node.directories) {
        result.push({ type: 'directory', directory, depth });
        if (!collapsed.has(directory.path)) visit(directory, depth + 1);
      }
      for (const item of node.files) result.push({ type: 'file', item, depth });
    };
    visit(tree, 0);
    return result;
  }, [items, tree, collapsed]);
  const toggleDirectory = useCallback((path: string) => setCollapsed(previous => {
    const next = new Set(previous);
    if (next.has(path)) next.delete(path); else next.add(path);
    return next;
  }), []);

  useLayoutEffect(() => {
    const element = host.current;
    const scrollParent = element?.closest<HTMLElement>('.aow-panel-viewport') ?? element?.closest<HTMLElement>('.changes-list, .commit-list');
    if (!element || !scrollParent || !active) return;
    viewport.current = scrollParent;
    const measure = () => {
      const box = scrollParent.getBoundingClientRect();
      const list = element.getBoundingClientRect();
      const shown = scrollParent.clientHeight > 0 && list.bottom > box.top && list.top < box.bottom;
      const start = shown ? Math.max(0, Math.floor((box.top - list.top) / rowHeight) - overscan) : 0;
      const end = shown ? Math.min(rows.length, Math.ceil((box.bottom - list.top) / rowHeight) + overscan) : 0;
      setRange(previous => previous.start === start && previous.end === end ? previous : { start, end });
    };
    const announceLayout = () => scrollParent.dispatchEvent(new Event(layoutEvent));
    const observer = new ResizeObserver(announceLayout);
    observer.observe(scrollParent);
    observer.observe(element);
    scrollParent.addEventListener('scroll', measure, { passive: true });
    scrollParent.addEventListener(layoutEvent, measure);
    scrollParent.addEventListener('aow-panel-layout', measure);
    announceLayout();
    return () => {
      observer.disconnect();
      scrollParent.removeEventListener('scroll', measure);
      scrollParent.removeEventListener(layoutEvent, measure);
      scrollParent.removeEventListener('aow-panel-layout', measure);
      viewport.current = null;
      // Removing an earlier commit's list moves its siblings without resizing
      // their viewports. Measure them after React finishes removing the DOM.
      queueMicrotask(announceLayout);
    };
  }, [active, rows]);

  useLayoutEffect(() => {
    if (pendingFocus.current === undefined) return;
    const button = host.current?.querySelector<HTMLButtonElement>(`[data-change-index="${pendingFocus.current}"] button`);
    if (button) {
      button.focus({ preventScroll: true });
      pendingFocus.current = undefined;
    }
  });

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || event.altKey || event.ctrlKey || event.metaKey) return;
    const element = (event.target as HTMLElement).closest<HTMLElement>('[data-change-index]');
    const scrollParent = viewport.current;
    if (!element || !scrollParent || !host.current || !rows.length) return;
    event.preventDefault();
    const index = Number(element.dataset.changeIndex);
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1
      : Math.max(0, Math.min(rows.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)));
    const box = scrollParent.getBoundingClientRect();
    const style = getComputedStyle(scrollParent);
    const start = box.top + parseFloat(style.getPropertyValue('--panel-scroll-top') || '0');
    const end = box.bottom - parseFloat(style.getPropertyValue('--panel-scroll-bottom') || '0');
    const top = host.current.getBoundingClientRect().top + next * rowHeight;
    if (top < start) scrollParent.scrollTop += top - start;
    else if (top + rowHeight > end) scrollParent.scrollTop += top + rowHeight - end;
    pendingFocus.current = next;
    scrollParent.dispatchEvent(new Event(layoutEvent));
    const button = host.current.querySelector<HTMLButtonElement>(`[data-change-index="${next}"] button`);
    if (button) {
      button.focus({ preventScroll: true });
      pendingFocus.current = undefined;
    }
  };

  return <div ref={host} className={`source-change-list${layout === 'tree' ? ' source-change-tree' : ''}`}
    style={{ height: rows.length * rowHeight }} onKeyDown={onKeyDown}>
    {active ? rows.slice(range.start, range.end).map((row, offset) => {
      const index = range.start + offset;
      return <div key={row.type === 'file' ? row.item.id : `directory:${row.directory.path}`}
        className="source-change-slot" data-change-index={index}
        style={{ top: index * rowHeight }}>
        {row.type === 'file'
          ? <FileRow item={row.item} compact={layout === 'tree'} depth={row.depth} selected={row.item.id === selectedId} onOpen={onOpen} />
          : <button className="tree-row change-folder-row" title={row.directory.path}
            style={{ '--tree-depth': row.depth } as CSSProperties}
            aria-expanded={!collapsed.has(row.directory.path)} onClick={() => toggleDirectory(row.directory.path)}>
            <span className={`chevron${collapsed.has(row.directory.path) ? '' : ' expanded'}`} aria-hidden="true"><ChevronRight /></span>
            <DirectoryTypeIcon expanded={!collapsed.has(row.directory.path)} />
            <span className="change-name">{row.directory.name}</span>
          </button>}
      </div>;
    }) : null}
  </div>;
});
