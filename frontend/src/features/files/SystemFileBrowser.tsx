import { t, getLanguage } from '../../lib/i18n';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { DragEvent, KeyboardEvent, MouseEvent } from 'react';
import { ArrowLeft, ArrowRight, Check, ChevronDown, ChevronRight, Home, LoaderCircle, Pin, PinOff, RefreshCw, Trash2, Upload, X } from 'lucide-react';
import { filesApi } from './api';
import { DirectoryTypeIcon, FileTypeIcon } from './FileTypeIcon';
import type { DirectoryListing, FileEntry } from './types';
import { usePinnedDirectories } from './usePinnedDirectories';
import { SystemFileContextMenu, type SystemFileMenuTarget } from './SystemFileContextMenu';
import './system-file-browser.css';

interface UploadItem {
  id: number;
  file: File;
  directory: string;
  loaded: number;
  status: 'waiting' | 'uploading' | 'success' | 'error';
  error?: string;
}

interface DirectoryTreeProps {
  path: string;
  depth?: number;
  currentPath: string;
  listings: Record<string, DirectoryListing>;
  expanded: Set<string>;
  pending: Set<string>;
  errors: Record<string, string>;
  onToggle: (path: string) => void;
  onNavigate: (path: string) => void;
  onRetry: (path: string) => void;
  selectedPath: string;
  onMenu: (event: MouseEvent | KeyboardEvent, path: string) => void;
}

function DirectoryPin({ path, pins }: {
  path: string; pins: ReturnType<typeof usePinnedDirectories>;
}) {
  const pinned = pins.paths.includes(path);
  const pending = pins.pending.has(path);
  const action = t(pinned ? "files.unpin_path" : "files.pin_path", { path });
  return <button type="button" className="system-file-pin" title={action} aria-label={action} aria-pressed={pinned}
    disabled={!path || pins.loading || pending} onClick={() => void pins.toggle(path, pinned)}>
    {pending ? <LoaderCircle className="system-file-spin" /> : pinned ? <PinOff /> : <Pin />}
  </button>;
}

const errorMessage = (reason: unknown) => reason instanceof Error ? reason.message : String(reason);
const parentPath = (path: string) => path.slice(0, path.lastIndexOf('/')) || '/';

function ancestors(path: string) {
  const paths = [path];
  while (path !== '/') { path = parentPath(path); paths.unshift(path); }
  return paths;
}

function formatSize(size: number) {
  if (size < 1024) return `${size} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let unit = -1;
  do { size /= 1024; unit += 1; } while (size >= 1024 && unit < units.length - 1);
  return `${size.toFixed(size < 10 ? 1 : 0)} ${units[unit]}`;
}

function DirectoryTree(props: DirectoryTreeProps) {
  const { path, depth = 0, currentPath, listings, expanded, pending, errors, onToggle, onNavigate, onRetry, selectedPath, onMenu } = props;
  const isExpanded = expanded.has(path);
  const directories = listings[path]?.entries.filter((entry) => entry.kind === 'directory');
  return <li>
    <div className={`system-file-tree-row${selectedPath === path ? ' selected' : ''}`} onContextMenu={(event) => onMenu(event, path)} onKeyDown={(event) => { if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) onMenu(event, path); }}>
      <button className="system-file-tree-toggle" style={{ marginLeft: depth * 13 }} aria-label={`${isExpanded ? t("common.collapse") : t("aow.expand")} ${path}`} aria-expanded={isExpanded} onClick={() => onToggle(path)}>
        {pending.has(path) ? <LoaderCircle className="system-file-spin" /> : isExpanded ? <ChevronDown /> : <ChevronRight />}
      </button>
      <button className="system-file-tree-name" title={path} aria-current={currentPath === path ? 'location' : undefined} onClick={(event) => { if (event.detail < 2) onNavigate(path); }} onDoubleClick={() => onToggle(path)}>
        <DirectoryTypeIcon expanded={isExpanded} /><span>{path === '/' ? '/' : path.split('/').pop()}</span>
      </button>
    </div>
    {isExpanded ? <ul>
      {errors[path] ? <li className="system-file-tree-error" style={{ paddingLeft: (depth + 1) * 13 + 8 }} title={errors[path]}>{errors[path]}<button onClick={() => onRetry(path)}>{t("aow.retry")}</button></li> : null}
      {directories?.map((entry) => <DirectoryTree key={entry.path} {...props} path={entry.path} depth={depth + 1} />)}
      {!pending.has(path) && !errors[path] && directories?.length === 0 ? <li className="system-file-tree-empty" style={{ paddingLeft: (depth + 1) * 13 + 8 }}>{t("files.no_subdirectories")}</li> : null}
    </ul> : null}
  </li>;
}

export function SystemFileBrowser({ open, onHide, onOpenFile, navigationRequest, onPathChange }: { open: boolean; onHide: () => void; onOpenFile: (entry: FileEntry) => void; navigationRequest?: { path: string }; onPathChange?: (path: string) => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const fileListRef = useRef<HTMLDivElement>(null);
  const treeRef = useRef<HTMLElement>(null);
  const scrollPositions = useRef({ tree: [0, 0], files: [0, 0] });
  useLayoutEffect(() => {
    if (!open) return;
    // Moving a portal container between worktrees can reset the DOM scroll offsets.
    for (const [element, position] of [[treeRef.current, scrollPositions.current.tree], [fileListRef.current, scrollPositions.current.files]] as const) {
      if (element) { element.scrollLeft = position[0]; element.scrollTop = position[1]; }
    }
  }, [open]);
  const revealPathRef = useRef('');
  const cacheRef = useRef<Record<string, DirectoryListing>>({});
  const requestsRef = useRef(new Map<string, Promise<DirectoryListing>>());
  const navigationRef = useRef(0);
  const queueRef = useRef<UploadItem[]>([]);
  const uploadingRef = useRef(false);
  const uploadIdRef = useRef(0);
  const dragDepthRef = useRef(0);
  const [home, setHome] = useState('');
  const [currentPath, setCurrentPath] = useState('');
  const [history, setHistory] = useState<{ paths: string[]; index: number }>({ paths: [], index: -1 });
  const [address, setAddress] = useState('');
  const [listings, setListings] = useState<Record<string, DirectoryListing>>({});
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(['/']));
  const [pending, setPending] = useState<Set<string>>(() => new Set());
  const [directoryErrors, setDirectoryErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [selectedPath, setSelectedPath] = useState('');
  const [contextMenu, setContextMenu] = useState<SystemFileMenuTarget | null>(null);
  const closeContextMenu = useCallback(() => setContextMenu(null), []);
  const [dragging, setDragging] = useState(false);
  const [uploads, setUploads] = useState<UploadItem[]>([]);
  const [revealRequest, setRevealRequest] = useState(0);
  const pins = usePinnedDirectories();

  const saveListing = useCallback((path: string, listing: DirectoryListing) => {
    cacheRef.current = { ...cacheRef.current, [path]: listing };
    setListings(cacheRef.current);
  }, []);

  const loadDirectory = useCallback(async (path: string, refresh = false): Promise<DirectoryListing> => {
    const pendingRequest = requestsRef.current.get(path);
    if (pendingRequest) {
      if (!refresh) return pendingRequest;
      await pendingRequest.catch(() => undefined);
    }
    if (!refresh && cacheRef.current[path]) return cacheRef.current[path];
    setPending((previous) => new Set(previous).add(path));
    setDirectoryErrors((previous) => ({ ...previous, [path]: '' }));
    const request: Promise<DirectoryListing> = filesApi.listDirectory(path).then((listing) => {
      if (requestsRef.current.get(path) === request) saveListing(path, listing);
      return listing;
    }).catch((reason: unknown) => {
      if (requestsRef.current.get(path) === request) setDirectoryErrors((previous) => ({ ...previous, [path]: errorMessage(reason) }));
      throw reason;
    }).finally(() => {
      if (requestsRef.current.get(path) !== request) return;
      requestsRef.current.delete(path);
      setPending((previous) => { const next = new Set(previous); next.delete(path); return next; });
    });
    requestsRef.current.set(path, request);
    return request;
  }, [saveListing]);

  const recordNavigation = useCallback((path: string, historyIndex?: number) => {
    setHistory((previous) => {
      if (historyIndex !== undefined) return { paths: previous.paths.map((entry, index) => index === historyIndex ? path : entry), index: historyIndex };
      if (previous.paths[previous.index] === path) return previous;
      const paths = [...previous.paths.slice(0, previous.index + 1), path];
      return { paths, index: paths.length - 1 };
    });
  }, []);

  const navigate = useCallback(async (path: string, refresh = false, historyIndex?: number) => {
    const generation = ++navigationRef.current;
    setLoading(true);
    setError('');
    setNotice('');
    try {
      const listing = await loadDirectory(path, refresh);
      if (generation !== navigationRef.current) return;
      recordNavigation(listing.path, historyIndex);
      setCurrentPath(listing.path);
      revealPathRef.current = listing.path;
      setRevealRequest((value) => value + 1);
      setAddress(listing.path);
      setSelectedPath((previous) => listing.entries.some((entry) => entry.path === previous) ? previous : '');
      if (fileListRef.current && listing.path !== currentPath) fileListRef.current.scrollTop = 0;
    } catch (reason) {
      if (generation === navigationRef.current) setError(t("files.unable_to_open", { value0: path, value1: errorMessage(reason) }));
    } finally {
      if (generation === navigationRef.current) setLoading(false);
    }
  }, [currentPath, loadDirectory, recordNavigation]);

  const loadHome = useCallback(async () => {
    const generation = ++navigationRef.current;
    setLoading(true);
    setError('');
    setNotice('');
    try {
      const listing = await filesApi.listHomeDirectory();
      setHome(listing.path);
      saveListing(listing.path, listing);
      if (generation !== navigationRef.current) return;
      recordNavigation(listing.path);
      setCurrentPath(listing.path);
      revealPathRef.current = listing.path;
      setRevealRequest((value) => value + 1);
      setAddress(listing.path);
      setSelectedPath('');
    } catch (reason) {
      if (generation === navigationRef.current) setError(t("files.unable_to_open_home", { value0: errorMessage(reason) }));
    } finally {
      if (generation === navigationRef.current) setLoading(false);
    }
  }, [saveListing, recordNavigation]);

  // Mounted on first use and kept alive when hidden, including DOM scroll positions and uploads.
  useEffect(() => {
    if (!navigationRequest) void loadHome();
    return () => { navigationRef.current += 1; };
  }, [loadHome]);

  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;
  useEffect(() => { if (navigationRequest) void navigateRef.current(navigationRequest.path); }, [navigationRequest]);
  useEffect(() => { if (currentPath) onPathChange?.(currentPath); }, [currentPath, onPathChange]);

  useEffect(() => {
    dragDepthRef.current = 0;
    setDragging(false);
    setContextMenu(null);
  }, [open]);

  useEffect(() => {
    if (!currentPath) return;
    // Reveal the selected directory without opening its own children.
    const paths = ancestors(currentPath).slice(0, -1);
    setExpanded((previous) => new Set([...previous, ...paths]));
    paths.forEach((path) => { void loadDirectory(path).catch(() => undefined); });
  }, [currentPath, loadDirectory, revealRequest]);

  useLayoutEffect(() => {
    const tree = treeRef.current;
    if (!open || !tree || !currentPath || revealPathRef.current !== currentPath) return;
    const name = tree.querySelector('[aria-current="location"]');
    const row = name?.parentElement;
    if (!name || !row) return;
    const bounds = tree.getBoundingClientRect();
    const rowBounds = row.getBoundingClientRect();
    const headingHeight = tree.querySelector('.system-file-pane-title')?.clientHeight ?? 0;
    if (rowBounds.top < bounds.top + headingHeight) tree.scrollTop += rowBounds.top - bounds.top - headingHeight;
    else if (rowBounds.bottom > bounds.bottom) tree.scrollTop += rowBounds.bottom - bounds.bottom;
    const nameStart = row.querySelector('.system-file-tree-toggle')?.getBoundingClientRect().left ?? name.getBoundingClientRect().left;
    const nameEnd = name.querySelector('span')?.getBoundingClientRect().right ?? nameStart;
    const visibleLeft = bounds.left + 5;
    const visibleRight = bounds.left + tree.clientWidth - 5;
    if (nameStart < visibleLeft || nameEnd > visibleRight) {
      tree.scrollLeft += nameStart < visibleLeft || nameEnd - nameStart > visibleRight - visibleLeft
        ? nameStart - visibleLeft : nameEnd - visibleRight;
    }
    revealPathRef.current = '';
  }, [open, currentPath, listings, expanded, revealRequest]);

  const toggleDirectory = (path: string) => {
    const opening = !expanded.has(path);
    setExpanded((previous) => {
      const next = new Set(previous);
      if (opening) next.add(path); else next.delete(path);
      return next;
    });
    if (opening) void loadDirectory(path).catch(() => undefined);
  };

  const showContextMenu = (event: MouseEvent | KeyboardEvent, path: string, entry?: FileEntry) => {
    event.preventDefault();
    event.stopPropagation();
    const bounds = event.currentTarget.getBoundingClientRect();
    event.currentTarget.querySelector<HTMLButtonElement>('.system-file-tree-name, .system-file-entry-name')?.focus({ preventScroll: true });
    setSelectedPath(path);
    setContextMenu({ path, entry, x: 'clientX' in event ? event.clientX : Math.max(bounds.left, treeRef.current?.getBoundingClientRect().left ?? 0) + 24, y: 'clientY' in event ? event.clientY : bounds.bottom });
  };

  const copyPath = async (path: string) => {
    setNotice('');
    try {
      try {
        if (!navigator.clipboard?.writeText) throw new Error(t("common.clipboard_unavailable"));
        await navigator.clipboard.writeText(path);
      } catch {
        const focused = document.activeElement;
        const input = document.createElement('textarea');
        input.value = path;
        input.readOnly = true;
        input.style.cssText = 'position:fixed;opacity:0;pointer-events:none';
        dialogRef.current!.appendChild(input);
        try {
          input.select();
          if (!document.execCommand('copy')) throw new Error(t("files.the_browser_denied_the_copy_operation"));
        } finally {
          input.remove();
          if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
        }
      }
      setNotice(t("files.copied", { value0: path }));
    } catch (reason) { setError(t("files.copy_failed", { value0: errorMessage(reason) })); }
  };

  const enqueueUploads = (files: File[]) => {
    if (!currentPath || loading || files.length === 0) return;
    const items: UploadItem[] = files.map((file) => ({ id: ++uploadIdRef.current, file, directory: currentPath, loaded: 0, status: 'waiting' }));
    queueRef.current.push(...items);
    setUploads((previous) => [...previous, ...items]);
    if (uploadingRef.current) return;
    uploadingRef.current = true;
    const update = (id: number, changes: Partial<UploadItem>) => setUploads((previous) => previous.map((item) => item.id === id ? { ...item, ...changes } : item));
    void (async () => {
      try {
        while (queueRef.current.length) {
          const item = queueRef.current.shift()!;
          update(item.id, { status: 'uploading' });
          try {
            await filesApi.uploadFile(item.directory, item.file, (loaded) => update(item.id, { loaded }));
            update(item.id, { status: 'success', loaded: item.file.size });
            try { await loadDirectory(item.directory, true); }
            catch (reason) { setError(t("files.file_uploaded_but_directory_refresh_failed", { value0: errorMessage(reason) })); }
          } catch (reason) { update(item.id, { status: 'error', error: errorMessage(reason) }); }
        }
      } finally { uploadingRef.current = false; }
    })();
  };

  const dropFiles = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    event.stopPropagation();
    dragDepthRef.current = 0;
    setDragging(false);
    const items = [...event.dataTransfer.items];
    const containsDirectory = items.some((item) => item.webkitGetAsEntry?.()?.isDirectory);
    if (containsDirectory) setError(t("files.drop_files_here_directory_uploads_are_not_supported"));
    const files = items.length
      ? items.filter((item) => item.kind === 'file' && !item.webkitGetAsEntry?.()?.isDirectory).map((item) => item.getAsFile()).filter((file): file is File => file !== null)
      : [...event.dataTransfer.files];
    enqueueUploads(files);
  };

  const listing = listings[currentPath];
  const activeUploads = uploads.filter((item) => item.status === 'waiting' || item.status === 'uploading').length;

  return <div ref={dialogRef} className="system-file-browser" hidden={!open} role="region" aria-label={t("aow.system_file_browser")}

    onKeyDown={(event) => { event.stopPropagation(); if (event.key === 'Escape') { event.preventDefault(); onHide(); } }}
    onClick={(event) => {
      if (event.target !== event.currentTarget) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onHide();
    }}
    onDragEnter={(event) => {
      if (!event.dataTransfer.types.includes('Files')) return;
      event.preventDefault(); event.stopPropagation();
      dragDepthRef.current += 1;
      setDragging(true);
    }}
    onDragOver={(event) => {
      if (!event.dataTransfer.types.includes('Files')) return;
      event.preventDefault(); event.stopPropagation();
      event.dataTransfer.dropEffect = currentPath && !loading ? 'copy' : 'none';
    }}
    onDragLeave={(event) => {
      event.preventDefault(); event.stopPropagation();
      dragDepthRef.current = Math.max(0, dragDepthRef.current - 1);
      if (!dragDepthRef.current) setDragging(false);
    }}
    onDrop={dropFiles}
  >
    <form className="system-file-toolbar system-file-address" onSubmit={(event) => {
      event.preventDefault();
      let path = address.trim();
      if (!path && !currentPath) { void loadHome(); return; }
      if (home && (path === '~' || path.startsWith('~/'))) path = home + path.slice(1);
      if (!path.startsWith('/')) { setError(t("files.enter_an_absolute_path_such_as_tmp_or_documents")); return; }
      void navigate(path.replace(/\/{2,}/g, '/').replace(/\/$/, '') || '/', true);
    }}>
      <button type="button" title={t("files.go_back")} aria-label={t("files.go_back")} disabled={loading || history.index <= 0} onClick={() => void navigate(history.paths[history.index - 1], true, history.index - 1)}><ArrowLeft /></button>
      <button type="button" title={t("files.go_forward")} aria-label={t("files.go_forward")} disabled={loading || history.index >= history.paths.length - 1} onClick={() => void navigate(history.paths[history.index + 1], true, history.index + 1)}><ArrowRight /></button>
      <input autoFocus aria-label={t("aow.directory_path")} spellCheck={false} value={address} placeholder={t("files.enter_directory_path")} onChange={(event) => setAddress(event.target.value)} />
      <button type="submit" disabled={loading}>{t("files.go")}</button>
      <button type="button" className="system-file-upload-button" title={t("files.upload_files")} aria-label={t("files.upload_files")} disabled={!currentPath || loading} onClick={() => fileInputRef.current?.click()}><Upload /></button>
      <input ref={fileInputRef} type="file" multiple hidden aria-label={t("files.choose_upload_files")} onChange={(event) => { enqueueUploads([...event.currentTarget.files ?? []]); event.currentTarget.value = ''; }} />
    </form>
    {error ? <div className="system-file-error" role="alert"><span>{error}</span><button aria-label={t("aow.dismiss_error")} onClick={() => setError('')}><X /></button></div> : null}
    <div className="system-file-body">
      <aside className="system-file-shortcuts" aria-label={t("files.quick_navigation_and_favorites")}>
        <div className="system-file-pane-title">{t("files.quick_navigation")}</div>
        <nav aria-label={t("files.quick_navigation")}>
          <button title={t("files.root")} onClick={() => void navigate('/')} aria-pressed={currentPath === '/'}><DirectoryTypeIcon /><span>/</span></button>
          <button title="/tmp" onClick={() => void navigate('/tmp')} aria-pressed={currentPath === '/tmp'}><DirectoryTypeIcon /><span>/tmp</span></button>
          <button title={home || t("navigation.home")} onClick={() => void (home ? navigate(home) : loadHome())} aria-pressed={!!home && currentPath === home}><Home /><span>{t("navigation.home")}</span></button>
        </nav>
        <section className="system-file-bookmarks" aria-label={t("files.favorite_directories")}>
          <div className="system-file-pane-title"><Pin /><span>{t("files.favorites_pins")}</span><button title={t("files.refresh_favorites")} aria-label={t("files.refresh_favorites")} disabled={pins.loading} onClick={() => void pins.reload()}><RefreshCw className={pins.loading ? 'system-file-spin' : ''} /></button></div>
          {pins.error ? <div className="system-file-bookmark-error" role="alert"><span>{pins.error}</span><button onClick={() => void pins.reload()}>{t("configuration.reload")}</button></div> : null}
          {pins.loading && pins.paths.length === 0 ? <p className="system-file-bookmarks-empty">{t("files.loading_favorites")}</p> : !pins.error && pins.paths.length === 0 ? <p className="system-file-bookmarks-empty">{t("files.pin_frequently_used_directories_for_quick_access_here")}</p> : null}
          <ul>{pins.paths.map((path) => <li key={path} className={currentPath === path ? 'selected' : ''}>
            <button className="system-file-bookmark-link" title={path} aria-label={t("files.open_favorite", { value0: path })} aria-current={currentPath === path ? 'location' : undefined} onClick={() => void navigate(path)}><DirectoryTypeIcon /><span><strong>{path.split('/').pop() || '/'}</strong><small>{path}</small></span></button>
            <DirectoryPin path={path} pins={pins} />
          </li>)}</ul>
        </section>
      </aside>
      <aside className="system-file-tree" ref={treeRef} onScroll={event => { if (open) scrollPositions.current.tree = [event.currentTarget.scrollLeft, event.currentTarget.scrollTop]; }} aria-label={t("files.directory_tree")}><div className="system-file-pane-title">{t("files.directory_tree")}</div><ul><DirectoryTree path="/" currentPath={currentPath} listings={listings} expanded={expanded} pending={pending} errors={directoryErrors} onToggle={toggleDirectory} onNavigate={(path) => void navigate(path)} onRetry={(path) => { void loadDirectory(path, true).catch(() => undefined); }} selectedPath={contextMenu?.path ?? currentPath} onMenu={showContextMenu} /></ul></aside>
      <div className="system-file-list" ref={fileListRef} onScroll={event => { if (open) scrollPositions.current.files = [event.currentTarget.scrollLeft, event.currentTarget.scrollTop]; }} aria-label={t("files.file_list")} aria-busy={loading}>
        <table><thead><tr><th>{t("automations.name")}</th><th>{t("files.size")}</th><th>{t("files.modified")}</th></tr></thead><tbody>
          {listing?.entries.map((entry) => <tr key={entry.path} className={selectedPath === entry.path ? 'selected' : ''} aria-selected={selectedPath === entry.path} onContextMenu={(event) => showContextMenu(event, entry.path, entry)} onKeyDown={(event) => {
            if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) showContextMenu(event, entry.path, entry);
            else if (event.key === 'Enter' && entry.kind === 'directory') { event.preventDefault(); void navigate(entry.path); }
          }} onClick={() => setSelectedPath(entry.path)} onDoubleClick={() => { if (entry.kind === 'directory') void navigate(entry.path); }}>
            <td><button className="system-file-entry-name" title={entry.is_symlink ? `${entry.path} → ${entry.link_target}` : entry.path}>
              {entry.kind === 'directory' ? <DirectoryTypeIcon /> : <FileTypeIcon path={entry.path} />}<span>{entry.name}</span>{entry.is_symlink ? <small>↗</small> : null}
            </button></td>
            <td>{entry.kind === 'file' ? formatSize(entry.size) : '—'}</td>
            <td>{entry.modified_ms == null ? '—' : new Date(entry.modified_ms).toLocaleString(getLanguage())}</td>
          </tr>)}
        </tbody></table>
        {!listing ? <div className="system-file-empty">{loading ? <><LoaderCircle className="system-file-spin" />{t("files.loading_directory")}</> : t("files.choose_a_directory")}</div> : listing.entries.length === 0 ? <div className="system-file-empty"><DirectoryTypeIcon expanded />{t("files.directory_is_empty_drop_files_to_upload")}</div> : null}
      </div>
    </div>
    {uploads.length > 0 ? <section className="system-file-uploads" aria-label={t("files.upload_tasks")}>
      <header><strong>{activeUploads ? t("files.uploading_files", { value0: activeUploads }) : t("files.upload_history")}</strong><span>{t("files.uploads_continue_when_this_window_is_hidden")}</span><button title={t("files.clear_finished")} aria-label={t("files.clear_finished")} onClick={() => setUploads((previous) => previous.filter((item) => item.status === 'waiting' || item.status === 'uploading'))}><Trash2 /></button></header>
      <ul>{uploads.map((item) => <li key={item.id}>
        {item.status === 'success' ? <Check className="system-file-success" /> : item.status === 'error' ? <X className="system-file-failure" /> : <LoaderCircle className={item.status === 'uploading' ? 'system-file-spin' : ''} />}
        <div title={`${item.directory.replace(/\/$/, '')}/${item.file.name}`}><strong>{item.file.name}</strong><small>{item.directory}</small></div>
        {item.status === 'uploading' ? <progress aria-label={t("files.upload_progress", { value0: item.file.name })} max={item.file.size || 1} value={item.loaded} /> : null}
        <span className={item.status === 'error' ? 'system-file-failure' : ''} title={item.error}>{item.status === 'waiting' ? t("files.waiting_to_upload") : item.status === 'success' ? t("files.uploaded") : item.status === 'error' ? item.error : item.loaded >= item.file.size ? t("files.saving") : `${Math.round(item.loaded / (item.file.size || 1) * 100)}%`}</span>
      </li>)}</ul>
    </section> : null}
    <footer className="system-file-footer"><span role="status" title={notice || currentPath}>{notice || (loading ? t("aow.loading") : t("files.items", { value0: listing?.entries.length ?? 0, value1: currentPath }))}</span><span>{t("files.drop_files_to_upload_to_the_current_directory")}</span></footer>
    {open && contextMenu ? <SystemFileContextMenu target={contextMenu} pins={pins} onClose={closeContextMenu} onCopy={(path) => void copyPath(path)} onOpenFile={onOpenFile} /> : null}
    {dragging ? <div className="system-file-drop-overlay"><Upload /><strong>{loading || !currentPath ? t("files.wait_for_the_directory_to_finish_loading") : t("files.release_to_upload_files")}</strong><span>{currentPath}</span></div> : null}
  </div>;
}
