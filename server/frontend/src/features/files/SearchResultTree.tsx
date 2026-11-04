import { t } from '../../lib/i18n';
import { useId, useMemo, useState, type CSSProperties, type ReactNode } from 'react';
import { ChevronRight, RefreshCw } from 'lucide-react';
import { AowPanel, usePanelListViewport } from '../../components/AowPanel';
import { AowIconButton } from '../../components/AowIconButton';
import { DirectoryTypeIcon, FileTypeIcon } from './FileTypeIcon';
import { writeExplorerPath } from './explorerDrag';
import { searchRows, searchTree, type SearchRow } from './searchTree';
import type { OpenSearchFile, SearchState } from './searchTypes';

interface Props {
  root: string; title: string; icon: ReactNode; source: 'project' | 'notes';
  state: SearchState; content: boolean; onOpen: OpenSearchFile; onRefresh: () => void;
}

export function SearchResultTree({ root, title, icon, source, state, content, onOpen, onRefresh }: Props) {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [panelCollapsed, setPanelCollapsed] = useState(false);
  const [selected, setSelected] = useState<string>();
  const id = useId();
  const tree = useMemo(() => searchTree(root, state.result?.files ?? []), [root, state.result]);
  const rows = useMemo(() => searchRows(tree, collapsed), [tree, collapsed]);
  const viewport = usePanelListViewport(rows.length, 24, !panelCollapsed);
  const toggle = (path: string) => setCollapsed(previous => {
    const next = new Set(previous); if (next.has(path)) next.delete(path); else next.add(path); return next;
  });
  const expandable = (row: SearchRow) => !row.match && (!row.node.file || row.node.file.matches.length > 0);
  const open = (row: SearchRow) => { if (row.node.file) onOpen(row.node.path, source, row.match); else toggle(row.node.path); };
  const select = (index: number) => { const row = rows[index]; if (row) { setSelected(row.id); viewport.reveal(index); } };
  const selectedIndex = Math.max(0, rows.findIndex(row => row.id === selected));
  return <AowPanel className="explorer explorer-section" title={title} icon={icon} collapsed={panelCollapsed} onCollapsedChange={setPanelCollapsed}
    actions={<AowIconButton title={t("files.refresh_search")} aria-label={t("files.refresh_search_2", { value0: title })} onClick={onRefresh}><RefreshCw /></AowIconButton>}>
    <div className="explorer-search-status" role="status">
      {state.loading ? t("files.searching") : state.error ? null : t("files.files", { count: state.result?.files.length ?? 0, value0: state.result?.files.length ?? 0, value1: content ? t("common.matches", { value0: state.result?.total_matches ?? 0 }) : '' })}
      {state.result?.truncated ? <span>{t("files.results_are_incomplete_because_a_count_time_or_output")}</span> : null}
    </div>
    {state.error ? <div className="side-error" role="alert">{state.error}</div> : null}
    {!state.loading && !state.error && !state.result?.files.length ? <div className="explorer-search-status">{t("files.no_matching_files")}</div> : null}
    {state.result?.files.length ? <div role="tree" tabIndex={0} aria-label={t("files.search_results", { value0: title })} aria-activedescendant={`${id}-${selectedIndex}`}
      onKeyDown={event => {
        const row = rows[selectedIndex];
        if (event.key === 'ArrowDown') select(Math.min(rows.length - 1, selectedIndex + 1));
        else if (event.key === 'ArrowUp') select(Math.max(0, selectedIndex - 1));
        else if (event.key === 'Home') select(0);
        else if (event.key === 'End') select(rows.length - 1);
        else if (event.key === 'ArrowRight') { if (expandable(row) && collapsed.has(row.node.path)) toggle(row.node.path); else select(Math.min(rows.length - 1, selectedIndex + 1)); }
        else if (event.key === 'ArrowLeft') { if (expandable(row) && !collapsed.has(row.node.path)) toggle(row.node.path); else select(rows.findIndex(item => item.id === row.parent)); }
        else if (event.key === 'Enter') open(row);
        else if (event.key === ' ' && expandable(row)) toggle(row.node.path);
        else return;
        event.preventDefault();
      }}>
      <div ref={viewport.host} className="aow-panel-virtual-list" style={{ height: rows.length * 24 }}>
        {rows.slice(viewport.start, viewport.end).map((row, offset) => {
          const index = viewport.start + offset;
          const expanded = expandable(row) ? !collapsed.has(row.node.path) : undefined;
          const match = row.match;
          const before = match ? Array.from(match.text.slice(0, match.preview_column).trimStart()) : [];
          return <div key={row.id} id={`${id}-${index}`} role="treeitem" aria-level={row.depth + 1} aria-expanded={expanded} aria-selected={index === selectedIndex}
            className={`tree-row explorer-search-row${selected === row.id ? ' selected' : ''}`} title={match ? `${row.node.path}:${match.line}:${match.column}\n${match.text}` : row.node.path}
            style={{ position: 'absolute', top: index * 24, height: 24, '--tree-depth': row.depth } as CSSProperties}
            draggable={Boolean(row.node.file)} onDragStart={event => writeExplorerPath(event.dataTransfer, row.node.path)}
            onClick={event => { setSelected(row.id); event.currentTarget.closest<HTMLElement>('[role=tree]')?.focus({ preventScroll: true }); open(row); }}>
            {match ? <><span className="explorer-match-line">{match.line}</span><span className="explorer-match-text">
              {before.length > 26 ? `…${before.slice(-26).join('')}` : before.join('')}<mark>{match.text.slice(match.preview_column, match.preview_column + match.preview_length)}</mark>{match.text.slice(match.preview_column + match.preview_length)}
            </span></> : <>
              <span className={`chevron${expanded ? ' expanded' : ''}`} onClick={event => { if (expandable(row)) { event.stopPropagation(); setSelected(row.id); event.currentTarget.closest<HTMLElement>('[role=tree]')?.focus({ preventScroll: true }); toggle(row.node.path); } }}>
                {expanded !== undefined ? <ChevronRight size={13} /> : null}
              </span>
              {row.node.file ? <FileTypeIcon path={row.node.path} /> : <DirectoryTypeIcon expanded={Boolean(expanded)} />}
              <span className="tree-label">{row.node.name}</span><span className="explorer-match-count">{row.node.count}</span>
            </>}
          </div>;
        })}
      </div>
    </div> : null}
  </AowPanel>;
}
