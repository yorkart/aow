import { useLayoutEffect, useRef, type RefObject } from 'react';
import type { TerminalFrame } from './terminalPresentation';

export function useMobileTerminalSurface(viewport: RefObject<HTMLDivElement | null>, frame: TerminalFrame, visible: boolean) {
  const surface = useRef<HTMLDivElement>(null);
  const currentFrame = useRef(frame);
  const updateSurface = useRef<() => void>(() => {});

  useLayoutEffect(() => {
    const clip = viewport.current;
    const node = surface.current;
    if (!clip || !node) return;
    const root = document.documentElement;
    let width = 0;
    let height = 0;
    const update = () => {
      const bounds = clip.getBoundingClientRect();
      if (!bounds.width || !bounds.height) return;
      const inset = parseFloat(root.style.getPropertyValue('--mobile-keyboard-inset')) || 0;
      // Pin the actual surface size throughout keyboard animation. Only the
      // clipping viewport shrinks; xterm's ResizeObserver sees no size change.
      if (!height || bounds.width !== width || inset === 0) {
        const safeArea = parseFloat(getComputedStyle(root).getPropertyValue('--mobile-keyboard-safe-area')) || 0;
        const nextHeight = Math.max(1, bounds.height + inset - safeArea);
        if (height !== nextHeight) node.style.height = `${nextHeight}px`;
        height = nextHeight;
        width = bounds.width;
      }
      const frame = currentFrame.current;
      const bottom = frame.top + frame.height;
      const cursorBottom = frame.top + (frame.cursorRow + 1) * frame.height / frame.rows;
      const offset = Math.max(0, Math.min(bottom - bounds.height, cursorBottom - bounds.height));
      // ResizeObserver runs before paint. Update the composited surface here,
      // without a React state update that can leave translation one frame late.
      // Whole device pixels also avoid changing glyph rasterization on the move.
      const dpr = window.devicePixelRatio || 1;
      const transform = `translate3d(0, ${-Math.round(offset * dpr) / dpr}px, 0)`;
      if (node.style.transform !== transform) node.style.transform = transform;
    };
    updateSurface.current = update;
    const observer = new ResizeObserver(update);
    observer.observe(clip);
    update();
    return () => { observer.disconnect(); updateSurface.current = () => {}; };
  }, [viewport]);

  useLayoutEffect(() => {
    currentFrame.current = frame;
    updateSurface.current();
  }, [frame, visible]);

  return surface;
}
