import { useEffect, useRef, type PointerEvent, type RefObject } from 'react';
import type { TerminalFrame, TerminalPaneControls } from './terminalPresentation';

type Point = { x: number; y: number };
type Gesture = { x: number; y: number; lastY: number; axis?: 'vertical' | 'horizontal'; remainder: number };
const TOUCH_SCROLL_MULTIPLIER = 2;

export function useMobileTerminalGestures(viewport: RefObject<HTMLDivElement | null>, controls: RefObject<TerminalPaneControls | null>,
  frame: TerminalFrame, visible: boolean, fontSize: number, onFontSizeChange: (size: number) => void) {
  const cellHeight = useRef(15);
  cellHeight.current = Math.max(1, frame.height / frame.rows);
  const zoom = useRef({ fontSize, onFontSizeChange });
  zoom.current = { fontSize, onFontSizeChange };
  const pointers = useRef(new Map<number, Point>());
  const pinch = useRef<{ distance: number; fontSize: number } | undefined>(undefined);
  const multiTouch = useRef(false);
  const gesture = useRef<Gesture | undefined>(undefined);
  const reset = () => { pointers.current.clear(); gesture.current = undefined; pinch.current = undefined; multiTouch.current = false; };
  useEffect(() => { reset(); return reset; }, [visible]);
  useEffect(() => {
    const node = viewport.current;
    if (!node || !visible) return;
    let remainder = 0;
    const wheel = (event: WheelEvent) => {
      // Non-bubbling events are the wheel inputs sent through xterm by our scroll control.
      if (!event.bubbles) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.ctrlKey) {
        zoom.current.onFontSizeChange(zoom.current.fontSize * Math.exp(-event.deltaY * .01));
        return;
      }
      if (event.shiftKey) return;
      const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? cellHeight.current : event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? node.clientHeight : 1;
      remainder += event.deltaY * unit / cellHeight.current;
      const lines = Math.trunc(remainder);
      if (lines) { controls.current?.scroll(lines, event); remainder -= lines; }
    };
    node.addEventListener('wheel', wheel, { capture: true, passive: false });
    return () => node.removeEventListener('wheel', wheel, { capture: true });
  }, [visible, viewport, controls]);
  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || !visible) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pointers.current.size >= 2) {
      multiTouch.current = true;
      gesture.current = undefined;
      const [first, second] = [...pointers.current.values()];
      pinch.current = { distance: Math.hypot(first.x - second.x, first.y - second.y), fontSize: zoom.current.fontSize };
    } else if (!multiTouch.current) {
      gesture.current = { x: event.clientX, y: event.clientY, lastY: event.clientY, remainder: 0 };
    }
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (!pointers.current.has(event.pointerId)) return;
    pointers.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pointers.current.size >= 2 && pinch.current && pinch.current.distance > 0) {
      const [first, second] = [...pointers.current.values()];
      const distance = Math.hypot(first.x - second.x, first.y - second.y);
      zoom.current.onFontSizeChange(pinch.current.fontSize * distance / pinch.current.distance);
      return;
    }
    const start = gesture.current;
    if (multiTouch.current || !start) return;
    const dx = start.x - event.clientX;
    const dy = start.y - event.clientY;
    if (!start.axis && Math.max(Math.abs(dx), Math.abs(dy)) < 4) return;
    start.axis ??= Math.abs(dx) > Math.abs(dy) ? 'horizontal' : 'vertical';
    if (start.axis === 'vertical') {
      start.remainder += (start.lastY - event.clientY) * TOUCH_SCROLL_MULTIPLIER / cellHeight.current;
      const lines = Math.trunc(start.remainder);
      if (lines) { controls.current?.scroll(lines, event); start.remainder -= lines; }
    }
    start.lastY = event.clientY;
  };
  const onPointerEnd = (event: PointerEvent<HTMLDivElement>) => {
    pointers.current.delete(event.pointerId);
    pinch.current = undefined;
    if (!pointers.current.size) { gesture.current = undefined; multiTouch.current = false; }
  };
  return { onPointerDown, onPointerMove, onPointerUp: onPointerEnd, onPointerCancel: onPointerEnd, onLostPointerCapture: onPointerEnd };
}
