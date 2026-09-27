import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { createPortal } from 'react-dom';
import { Clipboard, ClipboardPaste, Download, FilePlus2, FolderPlus, Pencil, Trash2 } from 'lucide-react';
import { WorkspaceMenuLabel } from '../../aow/WorkspaceMenuLabel';

interface Props {
  x: number;
  y: number;
  path: string;
  onClose: () => void;
  onOpenFloating?: () => void;
  onCopyFileName: () => void;
  onCopyRelativePath: () => void;
  onCopySystemPath: () => void;
  onPaste: () => void;
  onDownload?: () => void;
  onRename?: () => void;
  onCreateFile?: () => void;
  onCreateDirectory?: () => void;
  onDelete?: () => void;
}

export function ExplorerContextMenu({ x, y, path, onClose, onCopyFileName, onCopyRelativePath, onCopySystemPath, onPaste, onDownload, onRename, onCreateFile, onCreateDirectory, onDelete, onOpenFloating }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)),
    });
  }, [x, y]);

  useEffect(() => {
    const close = () => onClose();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', key);
    };
  }, [onClose]);

  const action = (callback: () => void) => (event: ReactMouseEvent) => {
    event.stopPropagation(); callback(); onClose();
  };

  return createPortal(
    <div ref={menu} className="project-aow-context-menu explorer-context-menu" style={{ left: position.x, top: position.y }} role="menu" aria-label={`${path} 操作`} onPointerDown={(event) => event.stopPropagation()}>
      {onOpenFloating ? <button role="menuitem" onClick={action(onOpenFloating)}><WorkspaceMenuLabel action="floating" /></button> : null}
      <button role="menuitem" onClick={action(onCopyFileName)}><Clipboard />复制 • 文件名</button>
      <button role="menuitem" onClick={action(onCopyRelativePath)}><Clipboard />复制 • 相对路径</button>
      <button role="menuitem" onClick={action(onCopySystemPath)}><Clipboard />复制 • 系统路径</button>
      <hr />
      <button role="menuitem" onClick={action(onPaste)}><ClipboardPaste />粘贴 • 文件|截图<kbd>{/Mac|iPhone|iPad/.test(navigator.platform) ? '⌘V' : 'Ctrl+V'}</kbd></button>
      {onCreateFile ? <button role="menuitem" onClick={action(onCreateFile)}><FilePlus2 />新建文件</button> : null}
      {onCreateDirectory ? <button role="menuitem" onClick={action(onCreateDirectory)}><FolderPlus />新建目录</button> : null}
      {onRename ? <button role="menuitem" onClick={action(onRename)}><Pencil />重命名</button> : null}
      {onDownload ? <button role="menuitem" onClick={action(onDownload)}><Download />下载</button> : null}
      {onDelete ? <button className="danger" role="menuitem" onClick={action(onDelete)}><Trash2 />删除</button> : null}
    </div>, document.body);
}
