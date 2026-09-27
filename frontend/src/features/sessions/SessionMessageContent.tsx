import { useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { sessionImageReferences } from './sessionImageReferences';
import { MarkdownContent } from '../../components/MarkdownContent';
import './session-image-preview.css';

function ImagePreview({ anchor, id, onClose, onEnter, onLeave }: {
  anchor: HTMLAnchorElement; id: string; onClose: () => void; onEnter: () => void; onLeave: () => void;
}) {
  const preview = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: 8, top: 8 });
  const [status, setStatus] = useState<'loading' | 'loaded' | 'error'>('loading');

  useLayoutEffect(() => {
    const bounds = preview.current?.getBoundingClientRect();
    if (!bounds) return;
    const rect = anchor.getBoundingClientRect();
    const top = rect.bottom + 8 + bounds.height <= window.innerHeight - 8
      ? rect.bottom + 8 : rect.top - bounds.height - 8;
    setPosition({
      left: Math.max(8, Math.min(rect.left, window.innerWidth - bounds.width - 8)),
      top: Math.max(8, Math.min(top, window.innerHeight - bounds.height - 8)),
    });
    anchor.setAttribute('aria-describedby', id);
    return () => anchor.removeAttribute('aria-describedby');
  }, [anchor, id]);

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
    window.addEventListener('keydown', keydown);
    window.addEventListener('scroll', onClose, true);
    window.addEventListener('resize', onClose);
    return () => {
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('scroll', onClose, true);
      window.removeEventListener('resize', onClose);
    };
  }, [onClose]);

  return createPortal(<div ref={preview} id={id} role="tooltip" className="session-image-preview" style={position}
    onMouseEnter={onEnter} onMouseLeave={onLeave}>
    <div className="session-image-preview-canvas">
      {status !== 'error' && <img src={anchor.href} alt={anchor.textContent ?? '图片预览'}
        className={status === 'loaded' ? 'loaded' : undefined} onLoad={() => setStatus('loaded')} onError={() => setStatus('error')} />}
      {status !== 'loaded' && <span role="status">{status === 'error' ? '图片无法加载，文件可能已移除或当前无法访问。' : '正在加载图片…'}</span>}
    </div>
    <div className="session-image-preview-caption">{anchor.textContent}</div>
  </div>, document.body);
}

export function SessionMessageContent({ text, imageReferenceText = text, className, previewImages = true }: {
  text: string; imageReferenceText?: string; className?: string; previewImages?: boolean;
}) {
  const images = useMemo(() => previewImages ? sessionImageReferences(imageReferenceText) : undefined, [imageReferenceText, previewImages]);
  const root = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [anchor, setAnchor] = useState<HTMLAnchorElement | null>(null);
  const id = useId();
  const cancelClose = useCallback(() => { clearTimeout(timer.current); }, []);
  const close = useCallback(() => { clearTimeout(timer.current); setAnchor(null); }, []);
  const scheduleClose = () => { cancelClose(); timer.current = setTimeout(close, 120); };

  useEffect(() => { close(); }, [text, images, close]);
  useEffect(() => cancelClose, [cancelClose]);

  const show = (target: EventTarget | null) => {
    // Portal events bubble through React even though the preview is outside the message.
    if (target instanceof Node && !root.current?.contains(target)) return;
    const link = target instanceof Element ? target.closest<HTMLAnchorElement>('a.session-image-reference') : null;
    if (!link || !root.current?.contains(link) || images?.get(link.dataset.sessionImage ?? '') !== link.getAttribute('href')) {
      scheduleClose();
      return;
    }
    cancelClose();
    setAnchor(link);
  };

  return <div ref={root} onMouseOver={(event) => show(event.target)} onMouseLeave={scheduleClose}
    onFocusCapture={(event) => show(event.target)} onBlurCapture={scheduleClose}>
    <MarkdownContent text={text} className={className} readOnly imageReferences={images} />
    {anchor && <ImagePreview key={anchor.href} anchor={anchor} id={id} onClose={close} onEnter={cancelClose} onLeave={scheduleClose} />}
  </div>;
}
