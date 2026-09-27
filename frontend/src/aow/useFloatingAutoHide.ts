import { useEffect, useRef, type RefObject } from 'react';

const hideDelay = 200;
const overlaySelector = '[role="menu"],[role="dialog"]:not([data-floating-workspace]),[role="alertdialog"],dialog[open],.project-aow-new-menu,.layout-menu,.monaco-menu-container';

function hasOpenOverlay() {
  return [...document.querySelectorAll<HTMLElement>(overlaySelector)].some(element =>
    element.getClientRects().length > 0 && getComputedStyle(element).visibility === 'visible');
}

export function useFloatingAutoHide(panel: RefObject<HTMLDivElement | null>, {
  open, pinned, autoHidden, showRequest, setAutoHidden,
}: {
  open: boolean;
  pinned: boolean;
  autoHidden: boolean;
  showRequest: number;
  setAutoHidden: (hidden: boolean) => void;
}) {
  const hidden = useRef(autoHidden);
  hidden.current = autoHidden;

  useEffect(() => {
    const element = panel.current;
    if (!open || pinned || !element) return;
    // Opening from the launcher or another workspace gives the user time to enter.
    let entered = element.matches(':hover');
    let point: { x: number; y: number } | undefined;
    let pressed = false;
    let dragging = false;
    let timer: number | undefined;
    const clear = () => { window.clearTimeout(timer); timer = undefined; };
    const schedule = () => {
      if (timer !== undefined) return;
      timer = window.setTimeout(() => { timer = undefined; check(true); }, hideDelay);
    };
    const check = (elapsed = false) => {
      if (pressed || dragging) { clear(); return; }
      const bounds = element.getBoundingClientRect();
      const inside = point && point.x >= bounds.left && point.x < bounds.right && point.y >= bounds.top && point.y < bounds.bottom;
      // The launcher remains an explicit toggle even if it overlaps the window.
      if (point && document.elementFromPoint(point.x, point.y)?.closest('.floating-workspace-trigger')) { clear(); return; }
      if (inside && !hidden.current) { entered = true; clear(); return; }
      if (!inside && (!entered || hidden.current)) { clear(); return; }
      // Menus can be portals outside the panel; native pickers can take browser focus.
      // Retry only while an overlay blocks hiding/revealing, including after Escape.
      if (!document.hasFocus() || hasOpenOverlay()
        || document.activeElement?.matches('.project-aow-tab-rename-input')) { schedule(); return; }
      if (inside) {
        entered = true;
        clear();
        hidden.current = false;
        setAutoHidden(false);
      } else if (elapsed) {
        hidden.current = true;
        setAutoHidden(true);
      } else schedule();
    };
    const move = (event: PointerEvent) => {
      if (event.pointerType !== 'mouse') return;
      point = { x: event.clientX, y: event.clientY };
      pressed = event.buttons !== 0;
      check();
    };
    const leave = (event: PointerEvent) => {
      if (event.pointerType !== 'mouse') return;
      point = undefined;
      check();
    };
    const startDrag = () => { dragging = true; clear(); };
    const endDrag = (event: DragEvent) => {
      dragging = false;
      pressed = false;
      point = { x: event.clientX, y: event.clientY };
      check();
    };
    const cancelPointer = () => { pressed = false; check(); };
    const focus = () => check();
    window.addEventListener('pointermove', move, true);
    window.addEventListener('pointerout', move, true);
    window.addEventListener('pointerdown', move, true);
    window.addEventListener('pointerup', move, true);
    window.addEventListener('pointercancel', cancelPointer, true);
    document.documentElement.addEventListener('pointerleave', leave);
    window.addEventListener('dragstart', startDrag, true);
    window.addEventListener('dragend', endDrag, true);
    window.addEventListener('drop', endDrag, true);
    window.addEventListener('focus', focus);
    return () => {
      clear();
      window.removeEventListener('pointermove', move, true);
      window.removeEventListener('pointerout', move, true);
      window.removeEventListener('pointerdown', move, true);
      window.removeEventListener('pointerup', move, true);
      window.removeEventListener('pointercancel', cancelPointer, true);
      document.documentElement.removeEventListener('pointerleave', leave);
      window.removeEventListener('dragstart', startDrag, true);
      window.removeEventListener('dragend', endDrag, true);
      window.removeEventListener('drop', endDrag, true);
      window.removeEventListener('focus', focus);
    };
  }, [open, pinned, showRequest, setAutoHidden, panel]);
}
