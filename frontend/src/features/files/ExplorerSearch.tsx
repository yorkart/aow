import { useMemo, useState, type ReactNode } from 'react';
import { CaseSensitive, Files, ListFilter, LoaderCircle, NotebookPen, Regex, Search, SlidersHorizontal, WholeWord, X } from 'lucide-react';
import { AowPanelStack } from '../../components/AowPanel';
import { useFileSearch } from './useFileSearch';
import { SearchResultTree } from './SearchResultTree';
import { ExplorerSearchButton } from './ExplorerSearchButton';
import type { OpenSearchFile, SearchOptions } from './searchTypes';
import './explorer-search.css';

interface Props {
  projectRoot: string; notesRoot: string; visible: boolean; onOpen: OpenSearchFile; children: ReactNode;
  projectRefresh: number; notesRefresh: number;
}

export function ExplorerSearch({ projectRoot, notesRoot, visible, onOpen, children, projectRefresh, notesRefresh }: Props) {
  const [query, setQuery] = useState('');
  const [composing, setComposing] = useState(false);
  const [options, setOptions] = useState<SearchOptions>({ mode: 'name', case_sensitive: false, whole_word: false, use_regex: false, include_ignored: false, include_pattern: '', exclude_pattern: '' });
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const content = options.mode === 'content';
  // Name filtering follows the existing tree, including ignored files.
  const effectiveOptions = useMemo(() => content ? options : { ...options, include_pattern: '', exclude_pattern: '', include_ignored: true }, [content, options]);
  const project = useFileSearch(projectRoot, query, effectiveOptions, visible && !composing, refresh + projectRefresh);
  const notes = useFileSearch(notesRoot, query, effectiveOptions, visible && !composing, refresh + notesRefresh);
  const searching = Boolean(query.trim());
  const toggle = (key: 'case_sensitive' | 'whole_word' | 'use_regex' | 'include_ignored') => setOptions(previous => ({ ...previous, [key]: !previous[key] }));
  return <div className="explorer-search">
    <div className="explorer-search-controls">
      <div className="explorer-search-input">
        <div className="explorer-search-query">
          {project.loading || notes.loading ? <LoaderCircle className="spinning" size={15} /> : content ? <Search size={15} /> : <ListFilter size={15} />}
          <input aria-label="搜索 Project 和 Notes" placeholder={content ? '搜索文件内容' : '按文件名或路径查找'} value={query} spellCheck={false}
            onChange={event => setQuery(event.currentTarget.value)} onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)}
            onKeyDown={event => { if (event.nativeEvent.isComposing) return; if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); setQuery(''); } if (event.key === 'Enter') setRefresh(value => value + 1); }} />
          {query ? <ExplorerSearchButton label="清空搜索" onClick={() => setQuery('')}><X size={14} /></ExplorerSearchButton> : null}
          {content ? <>
            <ExplorerSearchButton label="区分大小写" aria-pressed={options.case_sensitive} onClick={() => toggle('case_sensitive')}><CaseSensitive size={16} /></ExplorerSearchButton>
            <ExplorerSearchButton label="全词匹配" aria-pressed={options.whole_word} onClick={() => toggle('whole_word')}><WholeWord size={16} /></ExplorerSearchButton>
            <ExplorerSearchButton label="正则表达式" aria-pressed={options.use_regex} onClick={() => toggle('use_regex')}><Regex size={16} /></ExplorerSearchButton>
          </> : null}
        </div>
        <div className="explorer-search-modes" role="group" aria-label="搜索模式">
          <button aria-pressed={!content} onClick={() => setOptions(previous => ({ ...previous, mode: 'name' }))}>文件</button>
          <button aria-pressed={content} onClick={() => setOptions(previous => ({ ...previous, mode: 'content' }))}>内容</button>
          {content ? <ExplorerSearchButton className="explorer-search-filter-toggle" label="搜索范围" aria-expanded={filtersOpen} onClick={() => setFiltersOpen(value => !value)}><SlidersHorizontal size={14} /></ExplorerSearchButton> : null}
        </div>
      </div>
      {content && filtersOpen ? <div className="explorer-search-filters">
        <label>包含文件<input value={options.include_pattern} placeholder="*.ts, src/**" onChange={event => setOptions(previous => ({ ...previous, include_pattern: event.target.value }))} /></label>
        <label>排除文件<input value={options.exclude_pattern} placeholder="dist/**, *.min.js" onChange={event => setOptions(previous => ({ ...previous, exclude_pattern: event.target.value }))} /></label>
        <label className="explorer-search-check"><input type="checkbox" checked={options.include_ignored} onChange={() => toggle('include_ignored')} />包含被忽略的文件</label>
        <span>搜索磁盘上的文本文件（单文件最多 5 MiB）。</span>
      </div> : null}
    </div>
    <div className="explorer-search-body" hidden={searching}>{children}</div>
    {searching ? <AowPanelStack className="project-aow-explorer-stack explorer-search-results">
      <SearchResultTree key={`project:${project.key}`} root={projectRoot} title="Project Explorer" icon={<Files />} source="project" state={project} content={content} onOpen={onOpen} onRefresh={() => setRefresh(value => value + 1)} />
      <SearchResultTree key={`notes:${notes.key}`} root={notesRoot} title="Notes Explorer" icon={<NotebookPen />} source="notes" state={notes} content={content} onOpen={onOpen} onRefresh={() => setRefresh(value => value + 1)} />
    </AowPanelStack> : null}
  </div>;
}
