import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Copy, Download, FileText, LoaderCircle, Pin, PinOff } from 'lucide-react';
import { t } from '../../lib/i18n';
import { filesApi } from './api';
import type { FileEntry } from './types';
import type { usePinnedDirectories } from './usePinnedDirectories';

export interface SystemFileMenuTarget {
  x: number;
  y: number;
  path: string;
  entry?: FileEntry;
}

export function SystemFileContextMenu({ target, pins, onClose, onCopy, onOpenFile }: {
  target: SystemFileMenuTarget;
  pins: ReturnType<typeof usePinnedDirectories>;
  onClose: () => void;
  onCopy: (path: string) => void;
  onOpenFile: (entry: FileEntry) => void;
}) {
  const { x, y, path, entry } = target;
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });
  const directory = !entry || entry.kind === 'directory';
  const pinned = pins.paths.includes(path);
  const pending = pins.pending.has(path);
  const canOpen = entry?.kind === 'file' && !/\.(png|jpe?g|gif|webp|ico|bmp|pdf|zip|gz|tar|exe|so|woff2?|mp[34]|mov|docx?|xlsx?)$/i.test(entry.name);

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({ x: Math.max(4, Math.min(x, window.innerWidth - bounds.width - 4)), y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)) });
  }, [x, y]);

  useEffect(() => {
    const element = menu.current;
    const opener = document.activeElement;
    // A context click can follow scrollIntoView before its queued scroll event.
    // Dismiss only for scroll movement that actually happens after opening.
    const offsets = new Map<EventTarget, [number, number]>([[document, [window.scrollX, window.scrollY]]]);
    for (let ancestor = opener?.parentElement; ancestor; ancestor = ancestor.parentElement) offsets.set(ancestor, [ancestor.scrollLeft, ancestor.scrollTop]);
    element?.querySelector<HTMLElement>('[role="menuitem"]:not(:disabled)')?.focus({ preventScroll: true });
    const outside = (event: PointerEvent) => { if (!element?.contains(event.target as Node)) onClose(); };
    const scroll = (event: Event) => {
      if (element?.contains(event.target as Node)) return;
      const before = offsets.get(event.target!);
      const after = event.target instanceof Element ? [event.target.scrollLeft, event.target.scrollTop] : [window.scrollX, window.scrollY];
      if (!before || before[0] !== after[0] || before[1] !== after[1]) onClose();
    };
    window.addEventListener('pointerdown', outside);
    window.addEventListener('scroll', scroll, true);
    window.addEventListener('resize', onClose);
    return () => {
      window.removeEventListener('pointerdown', outside);
      window.removeEventListener('scroll', scroll, true);
      window.removeEventListener('resize', onClose);
      if ((element?.contains(document.activeElement) || document.activeElement === document.body) && opener instanceof HTMLElement && opener.isConnected && opener.getClientRects().length) opener.focus({ preventScroll: true });
    };
  }, [onClose, target]);

  const action = (callback: () => void) => { callback(); onClose(); };
  return createPortal(<div ref={menu} className="project-aow-context-menu system-file-context-menu" style={{ left: position.x, top: position.y }} role="menu" aria-label={t("aow.actions", { value0: path })}
    onContextMenu={(event) => event.preventDefault()}
    onKeyDown={(event) => {
      event.stopPropagation();
      if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); onClose(); return; }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      event.preventDefault();
      const items = [...menu.current!.querySelectorAll<HTMLElement>('[role="menuitem"]:not(:disabled)')];
      const index = items.indexOf(document.activeElement as HTMLElement);
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length;
      items[next]?.focus({ preventScroll: true });
    }}>
    {directory ? <button role="menuitem" disabled={pins.loading || pending} onClick={() => action(() => { void pins.toggle(path, pinned); })}>
      {pending ? <LoaderCircle className="system-file-spin" /> : pinned ? <PinOff /> : <Pin />}{t(pinned ? "files.unpin" : "files.pin")}
    </button> : null}
    {canOpen ? <button role="menuitem" onClick={() => action(() => onOpenFile(entry))}><FileText />{t("aow.open")}</button> : null}
    <button role="menuitem" onClick={() => action(() => onCopy(path))}><Copy />{t("files.copy_full_path")}</button>
    {entry?.kind === 'file' ? <a role="menuitem" href={filesApi.rawUrl(path)} download={entry.name} onClick={onClose}><Download />{t("files.download")}</a> : null}
  </div>, document.body);
}
