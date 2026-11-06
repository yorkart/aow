import { useRef, useState } from 'react';
import { t } from '../lib/i18n';

export function AgentToolsResizer({ controls, right, onResize }: {
  controls: string;
  right: boolean;
  onResize: (width: number) => void;
}) {
  const drag = useRef<{ pointer: number; x: number; width: number } | undefined>(undefined);
  const [dragging, setDragging] = useState(false);
  const resize = (handle: HTMLDivElement, width: number) => {
    const max = (handle.parentElement?.clientWidth ?? 0) * 0.6;
    onResize(Math.max(Math.min(150, max), Math.min(max, width)));
  };
  const stop = () => { drag.current = undefined; setDragging(false); };
  return <div className={`agent-tools-resizer${dragging ? ' dragging' : ''}`} role="separator"
    aria-label={t('aow.resize_details')} aria-controls={controls} aria-orientation="vertical" tabIndex={0}
    onPointerDown={event => {
      if (event.button !== 0) return;
      event.preventDefault();
      const width = event.currentTarget.previousElementSibling?.getBoundingClientRect().width;
      if (width === undefined) return;
      drag.current = { pointer: event.pointerId, x: event.clientX, width };
      event.currentTarget.setPointerCapture(event.pointerId);
      setDragging(true);
    }}
    onPointerMove={event => {
      const start = drag.current;
      if (start?.pointer !== event.pointerId) return;
      resize(event.currentTarget, start.width + (event.clientX - start.x) * (right ? -1 : 1));
    }}
    onPointerUp={event => {
      if (drag.current?.pointer !== event.pointerId) return;
      stop();
      event.currentTarget.releasePointerCapture(event.pointerId);
    }}
    onPointerCancel={stop} onLostPointerCapture={stop}
    onKeyDown={event => {
      if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return;
      event.preventDefault();
      const width = event.currentTarget.previousElementSibling?.getBoundingClientRect().width ?? 0;
      resize(event.currentTarget, width + (event.key === 'ArrowLeft' ? -20 : 20) * (right ? -1 : 1));
    }} />;
}
