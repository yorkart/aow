import { withFloatingOpen } from '../../aow/floatingWorkspaceState';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ClipboardEvent, MouseEvent, ReactNode } from 'react';
import { FixedSizeList, type ListChildComponentProps } from 'react-window';
import { ChevronRight, FolderUp, RefreshCw } from 'lucide-react';
import { filesApi } from './api';
import { gitApi } from '../git/api';
import { ExplorerContextMenu } from './ExplorerContextMenu';
import { DirectoryTypeIcon, FileTypeIcon } from './FileTypeIcon';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, usePanelCollapsed, usePanelListViewport } from '../../components/AowPanel';
import { isEditableTarget, pastedFiles, readClipboardImages } from './explorerClipboard';
import { useExplorerUploads } from './useExplorerUploads';
import type { FileEntry } from './types';

interface TreeNode extends FileEntry {
  depth: number; expanded: boolean; loading: boolean; ignored: boolean; children?: TreeNode[];
}

interface Props {
  root: string; activePath?: string;
  onRootChange: (path: string) => void; onOpenFile: (entry: FileEntry) => void;
  onRenameFile?: (path: string, name: string) => Promise<{ path: string; name: string }>;
  onCreateFile?: (directory: string, name: string) => Promise<unknown>;
  onCreateDirectory?: (directory: string, name: string) => Promise<unknown>;
  onDeleteEntry?: (path: string, directory: boolean) => void;
  refreshRequest: { generation: number; directory: string };
  lockedRoot?: boolean;
  titleCase?: boolean;
  title?: string;
  titleIcon?: ReactNode;
  aowHeader?: boolean;
  collapsed?: boolean;
  onCollapsedChange?: (collapsed: boolean) => void;
  renameDirectories?: boolean;
}

interface ContextMenuState { x: number; y: number; path: string; displayPath: string; directory: boolean; renameable: boolean }
interface RenameState { path: string; draft: string; busy: boolean }
interface CreationState { parent: string; kind: 'file' | 'directory'; draft: string; busy: boolean }
type DisplayRow = { type: 'root' } | { type: 'parent'; path: string } | { type: 'entry'; node: TreeNode } | { type: 'creation'; depth: number };
type VirtualRowData = { render: (props: ListChildComponentProps) => ReactNode; rows: DisplayRow[] };

// Keep the component identity and row keys stable during upload progress and
// directory refreshes, so an active rename input retains its draft and focus.
function VirtualRow(props: ListChildComponentProps<VirtualRowData>) {
  return props.data.render(props);
}

function virtualRowKey(index: number, data: VirtualRowData) {
  const row = data.rows[index];
  return row.type === 'entry' ? row.node.path : row.type;
}

function flatten(nodes: TreeNode[]): TreeNode[] {
  return nodes.flatMap((node) => [node, ...(node.expanded && node.children ? flatten(node.children) : [])]);
}

function updateNode(nodes: TreeNode[], path: string, update: (node: TreeNode) => TreeNode): TreeNode[] {
  return nodes.map((node) => node.path === path
    ? update(node)
    : node.children ? { ...node, children: updateNode(node.children, path, update) } : node);
}

function expandToDirectory(nodes: TreeNode[], directory: string): TreeNode[] {
  return nodes.map((node) => node.kind === 'directory' && (node.path === directory || directory.startsWith(`${node.path}/`))
    ? { ...node, expanded: true, children: node.children ? expandToDirectory(node.children, directory) : undefined }
    : node);
}

function preserveTreeState(nodes: TreeNode[], previousNodes: TreeNode[]): TreeNode[] {
  const previousByPath = new Map(previousNodes.map((node) => [node.path, node]));
  return nodes.map((node) => {
    const previous = previousByPath.get(node.path);
    if (node.kind !== 'directory' || previous?.kind !== 'directory') return node;
    return {
      ...node,
      expanded: previous.expanded,
      loading: node.children ? false : previous.loading,
      children: node.children ? preserveTreeState(node.children, previous.children ?? []) : previous.children,
    };
  });
}

async function loadTreeNodes(root: string, path: string, depth: number, previousNodes: TreeNode[] = []): Promise<TreeNode[]> {
  const listing = await filesApi.listDirectory(path);
  const entries = listing.entries.filter((entry) => entry.name !== '.git');
  const paths = entries.map((entry) => entry.path);
  const ignored = paths.length > 0
    ? await gitApi.gitIgnored(root, paths).then((result) => new Set(result.ignored)).catch(() => new Set<string>())
    : new Set<string>();
  const previousByPath = new Map(previousNodes.map((node) => [node.path, node]));
  return Promise.all(entries.map(async (entry) => {
    const previous = previousByPath.get(entry.path);
    const directory = entry.kind === 'directory' && previous?.kind === 'directory';
    return {
      ...entry,
      depth,
      expanded: directory && previous.expanded,
      loading: false,
      ignored: ignored.has(entry.path),
      // Refresh loaded branches too, preserving nested expansion under collapsed parents.
      children: directory && (previous.expanded || previous.children || previous.loading)
        ? await loadTreeNodes(root, entry.path, depth + 1, previous.children)
        : undefined,
    };
  }));
}

export function Explorer({ root, activePath, onRootChange, onOpenFile, onRenameFile, onCreateFile, onCreateDirectory, onDeleteEntry, refreshRequest, lockedRoot = false, titleCase = false, title, titleIcon, aowHeader = false, collapsed: controlledCollapsed, onCollapsedChange, renameDirectories = false }: Props) {
  const [nodes, setNodes] = useState<TreeNode[]>([]);
  const [error, setError] = useState('');
  const [refreshKey, setRefreshKey] = useState(0);
  const [height, setHeight] = useState(1);
  const [rootExpanded, setRootExpanded] = useState(true);
  const [rootLoading, setRootLoading] = useState(true);
  const [collapsed, changeCollapsed] = usePanelCollapsed(aowHeader && !rootLoading && !error && nodes.length === 0, controlledCollapsed, onCollapsedChange);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>();
  const [rename, setRename] = useState<RenameState>();
  const [creation, setCreation] = useState<CreationState>();
  const [status, setStatus] = useState('');
  const [selection, setSelection] = useState<{ path: string; directory: string }>();
  const [pasteHint, setPasteHint] = useState<{ directory: string; message: string }>();
  const [revealedPath, setRevealedPath] = useState<string>();
  const virtualList = useRef<FixedSizeList>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const fileInputDestination = useRef(root);
  const clipboardGeneration = useRef(0);
  const handledRefresh = useRef(0);
  const cancelRename = useRef(false);
  const cancelCreation = useRef(false);
  const treeList = useRef<HTMLDivElement>(null);
  const nodesRef = useRef(nodes);
  const loadedRoot = useRef(root);
  const treeGeneration = useRef(0);

  useLayoutEffect(() => { nodesRef.current = nodes; }, [nodes]);

  useLayoutEffect(() => {
    const element = treeList.current;
    if (!element) return;
    const resize = () => setHeight(Math.max(1, Math.floor(element.getBoundingClientRect().height)));
    resize();
    if (typeof ResizeObserver === 'undefined') {
      window.addEventListener('resize', resize);
      return () => window.removeEventListener('resize', resize);
    }
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    setRootExpanded(true); setRename(undefined); setCreation(undefined);
    setSelection(undefined); setPasteHint(undefined); setContextMenu(undefined); setRevealedPath(undefined);
    return () => { clipboardGeneration.current += 1; };
  }, [root]);

  useEffect(() => {
    let cancelled = false;
    const rootChanged = loadedRoot.current !== root;
    const previousNodes = rootChanged ? [] : nodesRef.current;
    loadedRoot.current = root;
    if (rootChanged) setNodes([]);
    setError(''); setRootLoading(true);
    loadTreeNodes(root, root, 0, previousNodes).then((entries) => {
      if (!cancelled) {
        setNodes((current) => preserveTreeState(entries, current));
        setRootLoading(false);
      }
    }).catch((reason: Error) => { if (!cancelled) { setError(reason.message); setRootLoading(false); } });
    return () => { cancelled = true; treeGeneration.current += 1; };
  }, [root, refreshKey]);

  const rows = useMemo(() => flatten(nodes), [nodes]);
  const parentPath = useMemo(() => {
    if (!root || root === '/') return undefined;
    const normalized = root.endsWith('/') ? root.slice(0, -1) : root;
    const index = normalized.lastIndexOf('/');
    return index <= 0 ? '/' : normalized.slice(0, index);
  }, [root]);
  const rootName = useMemo(() => {
    if (root === '/') return '/';
    const normalized = root.endsWith('/') ? root.slice(0, -1) : root;
    return normalized.split('/').pop() || root;
  }, [root]);
  const displayRows = useMemo<DisplayRow[]>(() => {
    const result: DisplayRow[] = [{ type: 'root' }];
    if (!rootExpanded) return result;
    if (parentPath && !lockedRoot) result.push({ type: 'parent', path: parentPath });
    if (creation?.parent === root) result.push({ type: 'creation', depth: 0 });
    const appendEntries = (entries: TreeNode[]) => {
      for (const node of entries) {
        result.push({ type: 'entry', node });
        if (creation?.parent === node.path) result.push({ type: 'creation', depth: node.depth + 1 });
        if (node.expanded && node.children) appendEntries(node.children);
      }
    };
    appendEntries(nodes);
    return result;
  }, [creation, lockedRoot, nodes, parentPath, root, rootExpanded]);
  const panelList = usePanelListViewport(displayRows.length, 24, aowHeader && !collapsed);
  const reloadDirectory = useCallback(async (path: string) => {
    const generation = treeGeneration.current;
    if (path === root) {
      const entries = await loadTreeNodes(root, root, 0, nodesRef.current);
      if (generation !== treeGeneration.current) return;
      setNodes((current) => preserveTreeState(entries, current));
      return;
    }
    const target = rows.find((node) => node.path === path);
    if (!target) { setRefreshKey((key) => key + 1); return; }
    const entries = await loadTreeNodes(root, path, target.depth + 1, target.children);
    if (generation !== treeGeneration.current) return;
    setNodes((current) => updateNode(current, path, (item) => ({
      ...item, loading: false,
      children: preserveTreeState(entries, item.children ?? []),
    })));
  }, [root, rows]);

  const upload = useExplorerUploads(root, async (directory, path) => {
    const generation = clipboardGeneration.current;
    const expanded = expandToDirectory(nodesRef.current, directory);
    setRootExpanded(true);
    setNodes(expanded);
    const entries = await loadTreeNodes(root, root, 0, expanded);
    if (generation !== clipboardGeneration.current) return;
    setNodes((current) => expandToDirectory(preserveTreeState(entries, current), directory));
    setRevealedPath(path);
  });
  const uploading = upload.uploads.some((item) => item.status === 'waiting' || item.status === 'uploading');

  useLayoutEffect(() => {
    if (!revealedPath) return;
    const index = displayRows.findIndex((row) => row.type === 'entry' && row.node.path === revealedPath);
    if (index >= 0) {
      if (aowHeader) panelList.reveal(index);
      else virtualList.current?.scrollToItem(index, 'smart');
    }
  }, [displayRows, revealedPath, aowHeader, panelList.reveal]);

  const enqueueFiles = (directory: string, files: File[]) => {
    setPasteHint(undefined);
    setError('');
    setStatus('');
    upload.enqueue(directory, files);
  };

  const paste = (event: ClipboardEvent<HTMLElement>) => {
    if (isEditableTarget(event.target) || collapsed || rename || creation) return;
    try {
      // Read File objects synchronously while the native paste data is available.
      const { files, hasDirectories } = pastedFiles(event.clipboardData);
      if (!files.length && !hasDirectories) {
        if (pasteHint) setPasteHint({ ...pasteHint, message: '剪贴板中没有可上传的文件或图片，请选择文件上传。' });
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const directory = pasteHint?.directory ?? selection?.directory ?? root;
      setContextMenu(undefined);
      enqueueFiles(directory, files);
      if (hasDirectories) setError('暂不支持整目录粘贴，请选择目录中的文件。');
    } catch (reason) { setError(reason instanceof Error ? reason.message : String(reason)); }
  };

  const pasteFromMenu = async (directory: string) => {
    const generation = clipboardGeneration.current;
    treeList.current?.focus({ preventScroll: true });
    setSelection({ path: directory, directory });
    setError('');
    setStatus('');
    setPasteHint(undefined);
    const fallback = (message: string) => {
      if (generation !== clipboardGeneration.current) return;
      setPasteHint({ directory, message });
      treeList.current?.focus({ preventScroll: true });
    };
    if (!window.isSecureContext || !navigator.clipboard?.read) {
      fallback('浏览器无法直接读取剪贴板，请按 Ctrl/Cmd+V 粘贴，或选择文件上传。');
      return;
    }
    try {
      const files = await readClipboardImages();
      if (generation !== clipboardGeneration.current) return;
      if (files.length) enqueueFiles(directory, files);
      else fallback('请按 Ctrl/Cmd+V 粘贴文件，或选择文件上传。');
    } catch {
      fallback('无法读取剪贴板，请按 Ctrl/Cmd+V 粘贴，或选择文件上传。');
    }
  };

  const showContextMenu = (event: MouseEvent, target: Omit<ContextMenuState, 'x' | 'y'>) => {
    event.preventDefault();
    event.stopPropagation();
    const directory = target.directory ? target.path : target.path.slice(0, target.path.lastIndexOf('/')) || '/';
    setSelection({ path: target.path, directory });
    setPasteHint(undefined);
    treeList.current?.focus({ preventScroll: true });
    setContextMenu({ ...target, x: event.clientX, y: event.clientY });
  };

  const beginRename = (path: string) => {
    cancelRename.current = false;
    setError('');
    setStatus('');
    setRename({ path, draft: path.split('/').filter(Boolean).pop() ?? path, busy: false });
  };

  const finishRename = async (requestedName: string) => {
    const current = rename;
    if (!current || current.busy) return;
    if (cancelRename.current) {
      cancelRename.current = false;
      setRename(undefined);
      return;
    }
    const nextName = requestedName.trim();
    const oldName = current.path.split('/').filter(Boolean).pop() ?? current.path;
    if (nextName === oldName) { setRename(undefined); return; }
    if (!onRenameFile) { setRename(undefined); return; }
    const pending = { ...current, draft: requestedName, busy: true };
    setRename(pending);
    setError('');
    try {
      const result = await onRenameFile(current.path, nextName);
      setRename(undefined);
      setStatus(`已重命名为 ${result.name}`);
    } catch (reason) {
      setRename({ ...pending, busy: false });
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const beginCreation = (parent: string, kind: CreationState['kind']) => {
    if (creation?.busy) return;
    cancelCreation.current = false;
    setError('');
    setStatus('');
    setRename(undefined);
    setCreation({ parent, kind, draft: '', busy: false });
    if (parent === root) {
      setRootExpanded(true);
      return;
    }
    const target = rows.find((node) => node.path === parent);
    if (!target || target.kind !== 'directory') return;
    setNodes((current) => updateNode(current, parent, (item) => ({ ...item, expanded: true })));
  };

  const finishCreation = async (requestedName: string) => {
    const current = creation;
    if (!current || current.busy) return;
    if (cancelCreation.current) {
      cancelCreation.current = false;
      setCreation(undefined);
      return;
    }
    const name = requestedName.trim();
    if (!name) {
      setCreation(undefined);
      return;
    }
    const create = current.kind === 'file' ? onCreateFile : onCreateDirectory;
    if (!create) {
      setCreation(undefined);
      return;
    }
    const pending = { ...current, draft: requestedName, busy: true };
    setCreation(pending);
    setError('');
    try {
      await create(current.parent, name);
      await reloadDirectory(current.parent);
      setCreation(undefined);
    } catch (reason) {
      setCreation({ ...pending, busy: false });
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  useEffect(() => {
    if (!refreshRequest.generation || refreshRequest.generation <= handledRefresh.current) return;
    handledRefresh.current = refreshRequest.generation;
    void reloadDirectory(refreshRequest.directory).catch((reason: Error) => setError(reason.message));
  }, [refreshRequest, reloadDirectory]);

  const copyText = useCallback(async (text: string) => {
    try {
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(text);
      } else {
        const input = document.createElement('textarea');
        input.value = text; input.readOnly = true; input.style.position = 'fixed'; input.style.opacity = '0';
        document.body.appendChild(input); input.select();
        if (!document.execCommand('copy')) throw new Error('浏览器拒绝复制操作');
        input.remove();
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }, []);

  const downloadFile = useCallback((path: string) => {
    const link = document.createElement('a');
    link.href = filesApi.rawUrl(path);
    link.download = path.split('/').filter(Boolean).pop() ?? 'download';
    document.body.appendChild(link);
    link.click();
    link.remove();
  }, []);

  const toggle = useCallback(async (node: TreeNode) => {
    if (node.kind !== 'directory') { onOpenFile(node); return; }
    if (node.expanded) {
      setNodes((current) => updateNode(current, node.path, (item) => ({ ...item, expanded: false }))); return;
    }
    if (node.children) {
      setNodes((current) => updateNode(current, node.path, (item) => ({ ...item, expanded: true }))); return;
    }
    setNodes((current) => updateNode(current, node.path, (item) => ({ ...item, expanded: true, loading: true })));
    const generation = treeGeneration.current;
    try {
      const entries = await loadTreeNodes(root, node.path, node.depth + 1);
      if (generation !== treeGeneration.current) return;
      setNodes((current) => updateNode(current, node.path, (item) => ({
        ...item, loading: false, children: entries,
      })));
    } catch (reason) {
      if (generation !== treeGeneration.current) return;
      setError(reason instanceof Error ? reason.message : String(reason));
      setNodes((current) => updateNode(current, node.path, (item) => ({ ...item, expanded: false, loading: false })));
    }
  }, [onOpenFile, root]);

  const Row = ({ index, style }: ListChildComponentProps) => {
    const row = displayRows[index];
    if (row.type === 'root') {
      return <div
        style={{ ...style, paddingLeft: 5 }}
        className="tree-row workspace-root-node"
        data-tree-path={root} data-tree-directory={root}
        title={root}
        onClick={() => setRootExpanded((value) => !value)}
        onContextMenu={(event) => showContextMenu(event, { path: root, displayPath: root, directory: true, renameable: false })}
      >
        <span className={`chevron ${rootExpanded ? 'expanded' : ''}`}><ChevronRight size={13} /></span>
        <DirectoryTypeIcon expanded={rootExpanded} />
        <span className="tree-label">{rootName}</span>
        {rootLoading ? <span className="tree-busy">…</span> : null}
      </div>;
    }
    if (row.type === 'parent') {
      return <div style={{ ...style, paddingLeft: 19 }} className="tree-row parent-workspace-node" title={`上级目录：${row.path}`} onClick={() => onRootChange(row.path)}>
        <span className="chevron" />
        <FolderUp className="parent-folder" />
        <span className="tree-label">..</span>
      </div>;
    }
    if (row.type === 'creation') {
      if (!creation) return null;
      const directory = creation.kind === 'directory';
      return <div
        style={{ ...style, paddingLeft: 19 + row.depth * 14 }}
        className="tree-row tree-new-entry"
        onClick={(event) => event.stopPropagation()}
        onPointerDown={(event) => event.stopPropagation()}
      >
        <span className="chevron" />
        {directory ? <DirectoryTypeIcon /> : <FileTypeIcon path="untitled" />}
        <input
          className="tree-rename-input"
          value={creation.draft}
          maxLength={255}
          autoFocus
          aria-label={directory ? '新目录名' : '新文件名'}
          disabled={creation.busy}
          onChange={(event) => {
            const draft = event.currentTarget.value;
            setCreation((current) => current ? { ...current, draft } : current);
          }}
          onClick={(event) => event.stopPropagation()}
          onDoubleClick={(event) => event.stopPropagation()}
          onContextMenu={(event) => event.stopPropagation()}
          onBlur={(event) => void finishCreation(event.currentTarget.value)}
          onKeyDown={(event) => {
            event.stopPropagation();
            if (event.key === 'Enter') event.currentTarget.blur();
            if (event.key === 'Escape') { cancelCreation.current = true; event.currentTarget.blur(); }
          }}
        />
        {creation.busy ? <span className="tree-busy">…</span> : null}
      </div>;
    }
    const node = row.node; const directory = node.kind === 'directory';
    return (
      <div
        style={{ ...style, paddingLeft: 19 + node.depth * 14 }}
        className={`tree-row${node.ignored ? ' git-ignored' : ''}${(selection?.path ?? activePath) === node.path ? ' selected' : ''}${revealedPath === node.path ? ' uploaded' : ''}`}
        data-tree-path={node.path} data-tree-directory={directory ? node.path : node.path.slice(0, node.path.lastIndexOf('/')) || '/'}
        title={node.path}
        onClick={() => toggle(node)}
        onContextMenu={(event) => showContextMenu(event, { path: node.path, displayPath: node.path, directory, renameable: node.kind === 'file' || (renameDirectories && directory) })}
      >
        <span className={`chevron ${directory && node.expanded ? 'expanded' : ''}`}>{directory ? <ChevronRight size={13} /> : null}</span>
        {directory ? <DirectoryTypeIcon expanded={node.expanded} /> : <FileTypeIcon path={node.path} />}
        {rename?.path === node.path ? <input
          className="tree-rename-input"
          defaultValue={rename.draft}
          maxLength={255}
          autoFocus
          aria-label="新文件名"
          disabled={rename.busy}
          onFocus={(event) => event.currentTarget.select()}
          onClick={(event) => event.stopPropagation()}
          onDoubleClick={(event) => event.stopPropagation()}
          onPointerDown={(event) => event.stopPropagation()}
          onContextMenu={(event) => event.stopPropagation()}
          onBlur={(event) => void finishRename(event.currentTarget.value)}
          onKeyDown={(event) => {
            event.stopPropagation();
            if (event.key === 'Enter') event.currentTarget.blur();
            if (event.key === 'Escape') { cancelRename.current = true; event.currentTarget.blur(); }
          }}
        /> : <span className="tree-label">{node.name}</span>}{node.loading || (rename?.path === node.path && rename.busy) ? <span className="tree-busy">…</span> : null}
      </div>
    );
  };

  const content = <>
      <div className="explorer-body" hidden={collapsed}>
        {!lockedRoot ? <div className="root-controls">
          <button title="系统根目录" onClick={() => onRootChange('/')}>/</button>
        </div> : null}
        {error ? <div className="side-error">{error}</div> : null}
        {status ? <div className="explorer-status">{status}</div> : null}
        {pasteHint ? <div className="explorer-paste-hint" role="status">
          <span>{pasteHint.message}</span><small title={pasteHint.directory}>上传到：{pasteHint.directory}</small>
          <button onClick={() => { fileInputDestination.current = pasteHint.directory; fileInput.current?.click(); }}>选择文件上传</button>
          <button aria-label="取消粘贴" onClick={() => setPasteHint(undefined)}>取消</button>
        </div> : null}
        <input ref={fileInput} type="file" multiple hidden aria-label="选择粘贴上传文件" onChange={(event) => {
          const files = Array.from(event.currentTarget.files ?? []);
          event.currentTarget.value = '';
          if (files.length && (fileInputDestination.current === root || fileInputDestination.current.startsWith(root === '/' ? '/' : `${root}/`))) {
            enqueueFiles(fileInputDestination.current, files);
          }
        }} />
        <div ref={treeList} className="tree-list" tabIndex={0} aria-label={`${title ?? 'Explorer'} 文件树`}
          onPointerDown={(event) => {
            if (isEditableTarget(event.target)) return;
            treeList.current?.focus({ preventScroll: true });
          }}
          onClick={(event) => {
            if (isEditableTarget(event.target)) return;
            const row = (event.target as HTMLElement).closest<HTMLElement>('[data-tree-path]');
            setSelection({ path: row?.dataset.treePath ?? root, directory: row?.dataset.treeDirectory ?? root });
            setPasteHint(undefined); setRevealedPath(undefined);
          }}
          onContextMenu={(event) => showContextMenu(event, { path: root, displayPath: root, directory: true, renameable: false })}
        >
          {aowHeader ? <div ref={panelList.host} className="aow-panel-virtual-list" style={{ height: displayRows.length * 24 }}>
            {displayRows.slice(panelList.start, panelList.end).map((row, offset) => {
              const index = panelList.start + offset;
              return <VirtualRow key={row.type === 'entry' ? row.node.path : row.type} index={index}
                style={{ position: 'absolute', top: index * 24, height: 24, width: '100%' }} data={{ render: Row, rows: displayRows }} />;
            })}
          </div> : <FixedSizeList ref={virtualList} height={height} width="100%" itemCount={displayRows.length} itemSize={24}
            itemData={{ render: Row, rows: displayRows }} itemKey={virtualRowKey}>{VirtualRow}</FixedSizeList>}
        </div>
        {upload.uploads.length ? <section className="explorer-uploads" aria-label="粘贴上传任务">
          <header><span>{uploading ? '正在上传…' : '上传记录'}</span>
            {uploading ? <button onClick={upload.cancel}>取消上传</button> : null}<button onClick={upload.clear}>清除已结束</button>
          </header>
          <ul>{upload.uploads.map((item) => <li key={item.id}>
            <span title={item.path ?? `${item.directory}/${item.file.name}`}>{item.path?.split('/').pop() ?? item.file.name}</span>
            {item.status === 'uploading' ? <progress aria-label={`上传进度 ${item.file.name}`} value={item.loaded} max={Math.max(1, item.file.size)} /> : null}
            <small role="status">{item.status === 'waiting' ? '等待上传' : item.status === 'success' ? '已上传' : item.status === 'error' ? '上传失败' : `${Math.min(100, Math.round(item.loaded / Math.max(1, item.file.size) * 100))}%`}</small>
            {item.status === 'error' ? <button onClick={() => upload.retry(item)}>重试</button> : null}
            {item.error ? <small className="explorer-upload-error">{item.error}</small> : null}
          </li>)}</ul>
        </section> : null}
      </div>
      {!collapsed && contextMenu ? <ExplorerContextMenu
        x={contextMenu.x}
        y={contextMenu.y}
        path={contextMenu.displayPath}
        onOpenFloating={contextMenu.directory ? undefined : () => {
          const node = rows.find(row => row.path === contextMenu.path);
          if (node) withFloatingOpen(() => onOpenFile(node));
        }}
        onClose={() => setContextMenu(undefined)}
        onCopyFileName={() => void copyText(contextMenu.path.split('/').filter(Boolean).pop() ?? '/')}
        onCopyRelativePath={() => void copyText(contextMenu.path.slice(root.replace(/\/+$/, '').length).replace(/^\/+/, '') || '.')}
        onCopySystemPath={() => void copyText(contextMenu.path)}
        onPaste={() => void pasteFromMenu(contextMenu.directory ? contextMenu.path : contextMenu.path.slice(0, contextMenu.path.lastIndexOf('/')) || '/')}
        onDownload={contextMenu.directory ? undefined : () => downloadFile(contextMenu.path)}
        onRename={contextMenu.renameable && onRenameFile ? () => beginRename(contextMenu.path) : undefined}
        onCreateFile={contextMenu.directory && onCreateFile ? () => beginCreation(contextMenu.path, 'file') : undefined}
        onCreateDirectory={contextMenu.directory && onCreateDirectory ? () => beginCreation(contextMenu.path, 'directory') : undefined}
        onDelete={contextMenu.path !== root && onDeleteEntry ? () => onDeleteEntry(contextMenu.path, contextMenu.directory) : undefined}
      /> : null}
    </>;
  if (aowHeader) return <AowPanel className="explorer explorer-section" title={title ?? 'Explorer'} icon={titleIcon}
    collapsed={collapsed} onCollapsedChange={changeCollapsed}
    actions={<AowIconButton title="刷新" aria-label={`刷新 ${title ?? 'Explorer'}`} onClick={() => setRefreshKey(key => key + 1)}><RefreshCw /></AowIconButton>}>
    <div onPaste={paste}>{content}</div>
  </AowPanel>;
  return <section className={`side-view explorer${onCollapsedChange ? ' explorer-section' : ''}${collapsed ? ' collapsed' : ''}`} onPaste={paste}>
    <div className="side-title">
      {onCollapsedChange ? <button className="explorer-section-toggle" type="button" aria-expanded={!collapsed} onClick={() => changeCollapsed(!collapsed)}>
        <ChevronRight className={collapsed ? undefined : 'expanded'} />{titleIcon}<strong>{title ?? (titleCase ? 'Explorer' : 'EXPLORER')}</strong>
      </button> : <strong>{title ?? (titleCase ? 'Explorer' : 'EXPLORER')}</strong>}
      <button className="explorer-refresh" title="刷新" aria-label={`刷新 ${title ?? 'Explorer'}`} onClick={() => setRefreshKey(key => key + 1)}><RefreshCw size={14} /></button>
    </div>{content}
  </section>;
}
