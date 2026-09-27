import type { Terminal } from '@xterm/xterm';

type CellPosition = [number, number] | undefined;
type DomRenderer = {
  renderRows(start: number, end: number): void;
  handleBlur(): void;
  handleSelectionChanged(start: CellPosition, end: CellPosition, column: boolean): void;
};

/** Keep native mobile input focus changes from rebuilding the entire terminal. */
export function installMobileTerminalRendering(terminal: Terminal): () => void {
  // xterm 6's DOM renderer redraws every row on blur and even empty selection
  // updates (a native click emits one). Its public API has no way to restrict
  // those repaints. Keep this compatibility shim local to the mobile DOM path;
  // fall back to normal rendering if a future xterm version changes its shape.
  const renderer = (terminal as Terminal & {
    _core?: { _renderService?: { _renderer?: { value?: Partial<DomRenderer> } } };
  })._core?._renderService?._renderer?.value;
  if (!terminal.element?.querySelector('.xterm-rows')
    || typeof renderer?.renderRows !== 'function'
    || typeof renderer.handleBlur !== 'function'
    || typeof renderer.handleSelectionChanged !== 'function') return () => {};

  const dom = renderer as DomRenderer;
  const originalBlur = dom.handleBlur;
  const originalSelection = dom.handleSelectionChanged;
  let renderedSelection = terminal.hasSelection();

  const limitRepaint = (action: () => void, cursorRow?: number) => {
    const renderRows = dom.renderRows;
    dom.renderRows = (start, end) => {
      if (cursorRow !== undefined && cursorRow >= 0 && cursorRow < terminal.rows
        && cursorRow >= start && cursorRow <= end) renderRows.call(dom, cursorRow, cursorRow);
    };
    try {
      action();
    } finally {
      dom.renderRows = renderRows;
    }
  };

  dom.handleBlur = () => {
    // Selected cells have different active/inactive colors; preserve their
    // normal repaint, including a selection whose clearing is still queued.
    if (renderedSelection || terminal.hasSelection()) {
      originalBlur.call(dom);
      return;
    }
    const buffer = terminal.buffer.active;
    limitRepaint(() => originalBlur.call(dom), buffer.baseY + buffer.cursorY - buffer.viewportY);
  };
  dom.handleSelectionChanged = (start, end, column) => {
    const hadSelection = renderedSelection;
    renderedSelection = !!(start && end && (start[0] !== end[0] || start[1] !== end[1]));
    const update = () => originalSelection.call(dom, start, end, column);
    // Still run xterm's selection bookkeeping, including clearing overlays.
    // Real selection changes and clearing an old highlight render normally.
    if (hadSelection || renderedSelection) update();
    else limitRepaint(update);
  };

  return () => {
    dom.handleBlur = originalBlur;
    dom.handleSelectionChanged = originalSelection;
  };
}
