import { t } from '../../lib/i18n';
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { parseTabUrl } from '../../aow/tabRoutes';
import { filesApi } from './api';
import './markdown-image-preview.css';

interface ImageLink { anchor: HTMLAnchorElement; path: string }

function imageLink(target: EventTarget | null, container: HTMLElement): ImageLink | undefined {
  const anchor = target instanceof Element ? target.closest('a') : null;
  if (!anchor || !container.contains(anchor)) return;
  const href = anchor.getAttribute('href') ?? '';
  // Only local destinations rewritten as file Tab links are eligible.
  if (!href.startsWith('/') || href.startsWith('//')) return;
  try {
    const file = parseTabUrl(new URL(href, window.location.origin));
    if (file?.type === 'file' && /\.(png|jpe?g|gif|webp|svg|bmp|ico|avif)$/i.test(file.path)) return { anchor, path: file.path };
  } catch { /* Invalid links must not interrupt the preview. */ }
}

function ImageTooltip({ id, link, onEnter, onLeave }: {
  id: string; link: ImageLink; onEnter: () => void; onLeave: () => void;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 0, top: 0 });
  const [status, setStatus] = useState<'loading' | 'loaded' | 'error'>('loading');
  const name = link.path.split('/').pop() ?? link.path;
  useLayoutEffect(() => {
    const bounds = panel.current?.getBoundingClientRect();
    if (!bounds) return;
    const anchor = link.anchor.getBoundingClientRect();
    const below = anchor.bottom + 8;
    setPosition({
      left: Math.max(8, Math.min(anchor.left, window.innerWidth - bounds.width - 8)),
      top: Math.max(8, Math.min(below + bounds.height <= window.innerHeight - 8 ? below : anchor.top - bounds.height - 8, window.innerHeight - bounds.height - 8)),
    });
  }, [link]);
  useEffect(() => {
    const previous = link.anchor.getAttribute('aria-describedby');
    link.anchor.setAttribute('aria-describedby', [previous, id].filter(Boolean).join(' '));
    return () => {
      if (previous === null) link.anchor.removeAttribute('aria-describedby');
      else link.anchor.setAttribute('aria-describedby', previous);
    };
  }, [id, link]);
  return createPortal(<div ref={panel} id={id} role="tooltip" aria-label={t("files.image_preview", { value0: name })} className="markdown-image-tooltip" style={position}
    onPointerEnter={onEnter} onPointerLeave={onLeave}>
    <div className="markdown-image-tooltip-frame">
      <img src={filesApi.rawUrl(link.path)} alt={name} hidden={status !== 'loaded'} onLoad={() => setStatus('loaded')} onError={() => setStatus('error')} />
      {status !== 'loaded' && <span role="status">{status === 'error' ? t("files.failed_to_load_image_preview") : t("files.loading_image")}</span>}
    </div>
    <div className="markdown-image-tooltip-name" title={link.path}>{name}</div>
  </div>, document.body);
}

export function MarkdownImagePreview({ children, content }: { children: ReactNode; content: string }) {
  const id = useId();
  const [link, setLink] = useState<ImageLink>();
  const openTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const cancelTimers = useCallback(() => { clearTimeout(openTimer.current); clearTimeout(closeTimer.current); }, []);
  const close = useCallback(() => { cancelTimers(); setLink(undefined); }, [cancelTimers]);
  const scheduleClose = useCallback(() => {
    cancelTimers(); closeTimer.current = setTimeout(close, 150);
  }, [cancelTimers, close]);
  useEffect(() => { close(); return cancelTimers; }, [content, close, cancelTimers]);
  useEffect(() => {
    if (!link) return;
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    document.addEventListener('keydown', keydown);
    document.addEventListener('pointerdown', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('keydown', keydown);
      document.removeEventListener('pointerdown', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  }, [close, link]);

  return <div className="markdown-image-preview-host" onPointerOver={event => {
    if (event.pointerType === 'touch') return;
    const next = imageLink(event.target, event.currentTarget);
    if (!next || (event.relatedTarget instanceof Node && next.anchor.contains(event.relatedTarget))) return;
    cancelTimers();
    if (next.anchor === link?.anchor) return;
    setLink(undefined);
    openTimer.current = setTimeout(() => setLink(next), 200);
  }} onPointerOut={event => {
    const current = imageLink(event.target, event.currentTarget);
    if (current && !(event.relatedTarget instanceof Node && current.anchor.contains(event.relatedTarget))) scheduleClose();
  }} onFocusCapture={event => {
    const next = imageLink(event.target, event.currentTarget);
    if (next?.anchor.matches(':focus-visible')) { cancelTimers(); setLink(next); }
  }} onBlurCapture={close} onClickCapture={close}>
    {children}
    {link && <ImageTooltip key={link.path} id={id} link={link} onEnter={cancelTimers} onLeave={scheduleClose} />}
  </div>;
}
