import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { DragEvent } from 'react';
import { ArrowUp, Check, ChevronDown, ChevronRight, Copy, Download, Home, LoaderCircle, Pin, PinOff, RefreshCw, Upload, X } from 'lucide-react';
import { filesApi } from './api';
import { DirectoryTypeIcon, FileTypeIcon } from './FileTypeIcon';
import type { DirectoryListing, FileEntry } from './types';
import { usePinnedDirectories } from './usePinnedDirectories';
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
  onCopy: (path: string) => void;
  pins: ReturnType<typeof usePinnedDirectories>;
}

function DirectoryPin({ path, label = path, pins, className = '', showLabel = false }: {
  path: string; label?: string; pins: ReturnType<typeof usePinnedDirectories>; className?: string; showLabel?: boolean;
}) {
  const pinned = pins.paths.includes(path);
  const pending = pins.pending.has(path);
  const action = `${pinned ? '取消 Pin' : 'Pin'} ${label}`;
  return <button type="button" className={`system-file-pin ${className}`} title={action} aria-label={action} aria-pressed={pinned}
    disabled={!path || pins.loading || pending} onClick={() => void pins.toggle(path, pinned)}>
    {pending ? <LoaderCircle className="system-file-spin" /> : pinned ? <PinOff /> : <Pin />}{showLabel ? <span>{pinned ? '取消 Pin' : 'Pin 当前目录'}</span> : null}
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
  const { path, depth = 0, currentPath, listings, expanded, pending, errors, onToggle, onNavigate, onRetry, onCopy, pins } = props;
  const isExpanded = expanded.has(path);
  const directories = listings[path]?.entries.filter((entry) => entry.kind === 'directory');
  return <li>
    <div className={`system-file-tree-row${currentPath === path ? ' selected' : ''}`}>
      <button className="system-file-tree-toggle" style={{ marginLeft: depth * 13 }} aria-label={`${isExpanded ? '折叠' : '展开'} ${path}`} aria-expanded={isExpanded} onClick={() => onToggle(path)}>
        {pending.has(path) ? <LoaderCircle className="system-file-spin" /> : isExpanded ? <ChevronDown /> : <ChevronRight />}
      </button>
      <button className="system-file-tree-name" title={path} aria-current={currentPath === path ? 'location' : undefined} onClick={() => onNavigate(path)}>
        <DirectoryTypeIcon expanded={isExpanded} /><span>{path === '/' ? '/' : path.split('/').pop()}</span>
      </button>
      <span className="system-file-tree-actions">
        <DirectoryPin path={path} pins={pins} className="system-file-tree-pin" />
        <button className="system-file-tree-copy" title={`复制路径 ${path}`} aria-label={`复制路径 ${path}`} onClick={() => onCopy(path)}><Copy /></button>
      </span>
    </div>
    {isExpanded ? <ul>
      {errors[path] ? <li className="system-file-tree-error" style={{ paddingLeft: (depth + 1) * 13 + 8 }} title={errors[path]}>{errors[path]}<button onClick={() => onRetry(path)}>重试</button></li> : null}
      {directories?.map((entry) => <DirectoryTree key={entry.path} {...props} path={entry.path} depth={depth + 1} />)}
      {!pending.has(path) && !errors[path] && directories?.length === 0 ? <li className="system-file-tree-empty" style={{ paddingLeft: (depth + 1) * 13 + 8 }}>无子目录</li> : null}
    </ul> : null}
  </li>;
}

export function SystemFileBrowser({ open, onHide, onOpenFile, navigationRequest, onPathChange }: { open: boolean; onHide: () => void; onOpenFile: (entry: FileEntry) => void; navigationRequest?: { path: string }; onPathChange?: (path: string) => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const fileListRef = useRef<HTMLDivElement>(null);
  const treeRef = useRef<HTMLElement>(null);
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
  const [address, setAddress] = useState('');
  const [listings, setListings] = useState<Record<string, DirectoryListing>>({});
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(['/']));
  const [pending, setPending] = useState<Set<string>>(() => new Set());
  const [directoryErrors, setDirectoryErrors] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const [selectedPath, setSelectedPath] = useState('');
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

  const navigate = useCallback(async (path: string, refresh = false) => {
    const generation = ++navigationRef.current;
    setLoading(true);
    setError('');
    setNotice('');
    try {
      const listing = await loadDirectory(path, refresh);
      if (generation !== navigationRef.current) return;
      setCurrentPath(listing.path);
      revealPathRef.current = listing.path;
      setRevealRequest((value) => value + 1);
      setAddress(listing.path);
      setSelectedPath((previous) => listing.entries.some((entry) => entry.path === previous) ? previous : '');
      if (fileListRef.current && listing.path !== currentPath) fileListRef.current.scrollTop = 0;
    } catch (reason) {
      if (generation === navigationRef.current) setError(`无法打开 ${path}：${errorMessage(reason)}`);
    } finally {
      if (generation === navigationRef.current) setLoading(false);
    }
  }, [currentPath, loadDirectory]);

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
      setCurrentPath(listing.path);
      revealPathRef.current = listing.path;
      setRevealRequest((value) => value + 1);
      setAddress(listing.path);
      setSelectedPath('');
    } catch (reason) {
      if (generation === navigationRef.current) setError(`无法打开 Home：${errorMessage(reason)}`);
    } finally {
      if (generation === navigationRef.current) setLoading(false);
    }
  }, [saveListing]);

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
  }, [open]);

  useEffect(() => {
    if (!currentPath) return;
    const paths = ancestors(currentPath);
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
    const actionsWidth = row.querySelector('.system-file-tree-actions')?.clientWidth ?? 0;
    const visibleRight = bounds.left + tree.clientWidth - 5 - actionsWidth;
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

  const copyPath = async (path: string) => {
    setNotice('');
    try {
      try {
        if (!navigator.clipboard?.writeText) throw new Error('Clipboard unavailable');
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
          if (!document.execCommand('copy')) throw new Error('浏览器拒绝复制操作');
        } finally {
          input.remove();
          if (focused instanceof HTMLElement) focused.focus({ preventScroll: true });
        }
      }
      setNotice(`已复制：${path}`);
    } catch (reason) { setError(`复制失败：${errorMessage(reason)}`); }
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
            catch (reason) { setError(`文件已上传，但目录刷新失败：${errorMessage(reason)}`); }
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
    if (containsDirectory) setError('请拖入文件；暂不支持整目录上传。');
    const files = items.length
      ? items.filter((item) => item.kind === 'file' && !item.webkitGetAsEntry?.()?.isDirectory).map((item) => item.getAsFile()).filter((file): file is File => file !== null)
      : [...event.dataTransfer.files];
    enqueueUploads(files);
  };

  const listing = listings[currentPath];
  const activeUploads = uploads.filter((item) => item.status === 'waiting' || item.status === 'uploading').length;

  return <div ref={dialogRef} className="system-file-browser" hidden={!open} role="region" aria-label="系统文件浏览器"

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
    <div className="system-file-toolbar">
      <div className="system-file-toolbar-actions">
        <button title="上级目录" aria-label="上级目录" disabled={!currentPath || currentPath === '/'} onClick={() => void navigate(parentPath(currentPath))}><ArrowUp /></button>
        <button title="刷新当前目录" aria-label="刷新当前目录" disabled={loading} onClick={() => void (currentPath ? navigate(currentPath, true) : loadHome())}><RefreshCw className={loading ? 'system-file-spin' : ''} /></button>
        <DirectoryPin path={currentPath} label="当前目录" pins={pins} showLabel />
        <button className="system-file-upload-button" disabled={!currentPath || loading} onClick={() => fileInputRef.current?.click()}><Upload />上传文件</button>
        <input ref={fileInputRef} type="file" multiple hidden aria-label="选择上传文件" onChange={(event) => { enqueueUploads([...event.currentTarget.files ?? []]); event.currentTarget.value = ''; }} />
      </div>
    </div>
    <form className="system-file-address" onSubmit={(event) => {
      event.preventDefault();
      let path = address.trim();
      if (home && (path === '~' || path.startsWith('~/'))) path = home + path.slice(1);
      if (!path.startsWith('/')) { setError('请输入绝对路径，例如 /tmp 或 ~/Documents'); return; }
      void navigate(path.replace(/\/{2,}/g, '/').replace(/\/$/, '') || '/');
    }}><input autoFocus aria-label="目录路径" spellCheck={false} value={address} placeholder="输入目录路径" onChange={(event) => setAddress(event.target.value)} /><button type="submit">前往</button><button type="button" title="复制当前目录路径" aria-label="复制当前目录路径" disabled={!currentPath} onClick={() => void copyPath(currentPath)}><Copy /></button></form>
    {error ? <div className="system-file-error" role="alert"><span>{error}</span><button aria-label="关闭错误提示" onClick={() => setError('')}><X /></button></div> : null}
    <div className="system-file-body">
      <aside className="system-file-shortcuts" aria-label="快捷导航与收藏">
        <div className="system-file-pane-title">快捷导航</div>
        <nav aria-label="快捷导航">
          <button title="根目录 /" onClick={() => void navigate('/')} aria-pressed={currentPath === '/'}><DirectoryTypeIcon /><span>/</span></button>
          <button title="/tmp" onClick={() => void navigate('/tmp')} aria-pressed={currentPath === '/tmp'}><DirectoryTypeIcon /><span>/tmp</span></button>
          <button title={home || 'Home'} onClick={() => void (home ? navigate(home) : loadHome())} aria-pressed={!!home && currentPath === home}><Home /><span>Home</span></button>
        </nav>
        <section className="system-file-bookmarks" aria-label="收藏目录">
          <div className="system-file-pane-title"><Pin /><span>收藏 / Pin</span><button title="刷新收藏" aria-label="刷新收藏" disabled={pins.loading} onClick={() => void pins.reload()}><RefreshCw className={pins.loading ? 'system-file-spin' : ''} /></button></div>
          {pins.error ? <div className="system-file-bookmark-error" role="alert"><span>{pins.error}</span><button onClick={() => void pins.reload()}>重新加载</button></div> : null}
          {pins.loading && pins.paths.length === 0 ? <p className="system-file-bookmarks-empty">正在加载收藏…</p> : !pins.error && pins.paths.length === 0 ? <p className="system-file-bookmarks-empty">Pin 常用目录，在这里快速访问。</p> : null}
          <ul>{pins.paths.map((path) => <li key={path} className={currentPath === path ? 'selected' : ''}>
            <button className="system-file-bookmark-link" title={path} aria-label={`打开收藏 ${path}`} aria-current={currentPath === path ? 'location' : undefined} onClick={() => void navigate(path)}><DirectoryTypeIcon /><span><strong>{path.split('/').pop() || '/'}</strong><small>{path}</small></span></button>
            <DirectoryPin path={path} pins={pins} />
          </li>)}</ul>
        </section>
      </aside>
      <aside className="system-file-tree" ref={treeRef} aria-label="目录树"><div className="system-file-pane-title">目录树</div><ul><DirectoryTree path="/" currentPath={currentPath} listings={listings} expanded={expanded} pending={pending} errors={directoryErrors} onToggle={toggleDirectory} onNavigate={(path) => void navigate(path)} onRetry={(path) => { void loadDirectory(path, true).catch(() => undefined); }} onCopy={(path) => void copyPath(path)} pins={pins} /></ul></aside>
      <div className="system-file-list" ref={fileListRef} aria-label="文件列表" aria-busy={loading}>
        <table><thead><tr><th>名称</th><th>大小</th><th>修改时间</th><th>操作</th></tr></thead><tbody>
          {listing?.entries.map((entry) => <tr key={entry.path} className={selectedPath === entry.path ? 'selected' : ''} aria-selected={selectedPath === entry.path} onClick={() => setSelectedPath(entry.path)} onDoubleClick={() => { if (entry.kind === 'directory') void navigate(entry.path); }}>
            <td><button className="system-file-entry-name" title={entry.is_symlink ? `${entry.path} → ${entry.link_target}` : entry.path} onClick={() => { setSelectedPath(entry.path); if (entry.kind === 'directory') void navigate(entry.path); }}>
              {entry.kind === 'directory' ? <DirectoryTypeIcon /> : <FileTypeIcon path={entry.path} />}<span>{entry.name}</span>{entry.is_symlink ? <small>↗</small> : null}
            </button></td>
            <td>{entry.kind === 'file' ? formatSize(entry.size) : '—'}</td>
            <td>{entry.modified_ms == null ? '—' : new Date(entry.modified_ms).toLocaleString()}</td>
            <td><div className="system-file-entry-actions">{entry.kind === 'file' && !/\.(png|jpe?g|gif|webp|ico|bmp|pdf|zip|gz|tar|exe|so|woff2?|mp[34]|mov|docx?|xlsx?)$/i.test(entry.name) ? <button title="打开文件" aria-label={`打开 ${entry.name}`} onClick={() => onOpenFile(entry)}>打开</button> : null}<button title="复制路径" aria-label={`复制路径 ${entry.name}`} onClick={() => void copyPath(entry.path)}><Copy /></button>{entry.kind === 'directory' ? <DirectoryPin path={entry.path} label={entry.name} pins={pins} /> : entry.kind === 'file' ? <a href={filesApi.rawUrl(entry.path)} download={entry.name} title="下载文件" aria-label={`下载 ${entry.name}`}><Download /></a> : null}</div></td>
          </tr>)}
        </tbody></table>
        {!listing ? <div className="system-file-empty">{loading ? <><LoaderCircle className="system-file-spin" />正在加载目录…</> : '请选择目录'}</div> : listing.entries.length === 0 ? <div className="system-file-empty"><DirectoryTypeIcon expanded />目录为空，可拖入文件上传</div> : null}
      </div>
    </div>
    {uploads.length > 0 ? <section className="system-file-uploads" aria-label="上传任务">
      <header><strong>{activeUploads ? `正在上传 · ${activeUploads} 个文件` : '上传记录'}</strong><span>隐藏窗口后继续上传</span><button onClick={() => setUploads((previous) => previous.filter((item) => item.status === 'waiting' || item.status === 'uploading'))}>清除已结束</button></header>
      <ul>{uploads.map((item) => <li key={item.id}>
        {item.status === 'success' ? <Check className="system-file-success" /> : item.status === 'error' ? <X className="system-file-failure" /> : <LoaderCircle className={item.status === 'uploading' ? 'system-file-spin' : ''} />}
        <div title={`${item.directory.replace(/\/$/, '')}/${item.file.name}`}><strong>{item.file.name}</strong><small>{item.directory}</small></div>
        {item.status === 'uploading' ? <progress aria-label={`上传进度 ${item.file.name}`} max={item.file.size || 1} value={item.loaded} /> : null}
        <span className={item.status === 'error' ? 'system-file-failure' : ''} title={item.error}>{item.status === 'waiting' ? '等待上传' : item.status === 'success' ? '已上传' : item.status === 'error' ? item.error : item.loaded >= item.file.size ? '正在保存…' : `${Math.round(item.loaded / (item.file.size || 1) * 100)}%`}</span>
      </li>)}</ul>
    </section> : null}
    <footer className="system-file-footer"><span role="status" title={notice || currentPath}>{notice || (loading ? '正在加载…' : `${listing?.entries.length ?? 0} 个项目 · ${currentPath}`)}</span><span>拖入文件上传到当前目录</span></footer>
    {dragging ? <div className="system-file-drop-overlay"><Upload /><strong>{loading || !currentPath ? '请等待目录加载完成' : '松开以上传文件'}</strong><span>{currentPath}</span></div> : null}
  </div>;
}
