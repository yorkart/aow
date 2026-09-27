import { createContext, useCallback, useContext, useId, useLayoutEffect, useRef, useState, type HTMLAttributes, type ReactNode } from 'react';
import { AowPanelHeader, type AowPanelHeaderProps } from './AowPanelHeader';
import './aow-panel.css';

const PanelStackContext = createContext<((anchor: HTMLElement) => void) | undefined>(undefined);

function panelAnchors(viewport: HTMLElement) {
  return [...viewport.querySelectorAll<HTMLElement>('[data-panel-anchor]')]
    .filter(anchor => anchor.closest('.aow-panel-viewport') === viewport && anchor.getClientRects().length && anchor.offsetHeight > 0);
}

// Keep the real header and its placeholder. Docked headers use the stack as
// their fixed-position containing block, so the browser holds them still even
// when scrolling is composited before JavaScript can update the layout.
export function AowPanelStack({ children, className = '', ...props }: HTMLAttributes<HTMLDivElement>) {
  const viewport = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [overflowPanels, setOverflowPanels] = useState<{ id: string; title: string }[]>([]);
  const crowded = useRef(false);
  const navigate = useCallback((anchor: HTMLElement) => {
    requestAnimationFrame(() => {
      const scroller = viewport.current;
      if (!scroller || !anchor.isConnected) return;
      const anchors = panelAnchors(scroller);
      const previous = crowded.current ? 0 : anchors.slice(0, anchors.indexOf(anchor)).reduce((sum, item) => sum + item.offsetHeight, 0);
      const top = scroller.scrollTop + anchor.getBoundingClientRect().top - scroller.getBoundingClientRect().top - previous;
      scroller.scrollTo({ top: Math.max(0, top), behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' });
    });
  }, []);

  useLayoutEffect(() => {
    const scroller = viewport.current!;
    let frame = 0;
    let disposed = false;
    const measure = () => {
      frame = 0;
      if (!scroller.clientHeight) return;
      const anchors = panelAnchors(scroller);
      const bounds = scroller.getBoundingClientRect();
      const heights = anchors.map(anchor => anchor.offsetHeight);
      const positions = anchors.map(anchor => anchor.getBoundingClientRect().top - bounds.top);
      const total = heights.reduce((sum, height) => sum + height, 0);
      // Many projects or a short window must still leave room to read content.
      // Use one compact jump list when all the docked bars would fill the view.
      crowded.current = total > scroller.clientHeight - Math.min(100, scroller.clientHeight / 2);
      const next = crowded.current ? anchors.map(anchor => ({ id: anchor.id, title: anchor.dataset.panelTitle! })) : [];
      setOverflowPanels(previous => JSON.stringify(previous) === JSON.stringify(next) ? previous : next);
      scroller.style.setProperty('--panel-viewport-top', `${scroller.offsetTop}px`);
      scroller.style.setProperty('--panel-viewport-left', `${scroller.offsetLeft}px`);
      scroller.style.setProperty('--panel-viewport-width', `${scroller.clientWidth}px`);
      let preceding = 0;
      let topInset = 0;
      let bottomInset = 0;
      for (const [index, anchor] of anchors.entries()) {
        const header = anchor.firstElementChild as HTMLElement;
        header.style.removeProperty('transform');
        const naturalTop = positions[index];
        const bottom = scroller.clientHeight - total + preceding;
        const dock = crowded.current ? '' : naturalTop < preceding ? 'top' : naturalTop > bottom ? 'bottom' : '';
        if (header.dataset.docked !== dock) header.dataset.docked = dock;
        if (dock) {
          const offset = `${dock === 'top' ? preceding : bottom}px`;
          if (header.style.getPropertyValue('--panel-dock-offset') !== offset) header.style.setProperty('--panel-dock-offset', offset);
        }
        if (dock === 'top') topInset = preceding + heights[index];
        if (dock === 'bottom') bottomInset = Math.max(bottomInset, total - preceding);
        preceding += heights[index];
      }
      scroller.style.setProperty('--panel-scroll-top', `${topInset}px`);
      scroller.style.setProperty('--panel-scroll-bottom', `${bottomInset}px`);
      scroller.dispatchEvent(new Event('aow-panel-layout'));
    };
    const schedule = () => { if (!disposed && !frame) frame = requestAnimationFrame(measure); };
    const resize = new ResizeObserver(schedule);
    resize.observe(scroller);
    resize.observe(content.current!);
    const mutations = new MutationObserver(schedule);
    mutations.observe(content.current!, { childList: true, subtree: true, attributes: true, attributeFilter: ['hidden', 'class', 'data-panel-title'] });
    scroller.addEventListener('scroll', schedule, { passive: true });
    measure();
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
      resize.disconnect(); mutations.disconnect();
      scroller.removeEventListener('scroll', schedule);
    };
  }, []);

  return <PanelStackContext.Provider value={navigate}>
    <div {...props} className={`aow-panel-stack ${className}`}>
      {overflowPanels.length ? <select className="aow-panel-jump" aria-label="快速定位面板" value="" onChange={event => {
        const anchor = document.getElementById(event.target.value);
        anchor?.querySelector<HTMLButtonElement>('.aow-panel-header-title')?.click();
      }}><option value="" disabled>快速定位面板</option>{overflowPanels.map(panel => <option key={panel.id} value={panel.id}>{panel.title}</option>)}</select> : null}
      <div ref={viewport} className="aow-panel-viewport">
        <div ref={content} className="aow-panel-content">{children}</div>
      </div>
    </div>
  </PanelStackContext.Provider>;
}

interface Props extends Omit<AowPanelHeaderProps, 'onToggle' | 'onNavigate' | 'children' | 'className'> {
  children: ReactNode;
  className?: string;
  headerClassName?: string;
  bodyClassName?: string;
  bodyRole?: 'list';
  empty?: boolean;
  onCollapsedChange?: (collapsed: boolean) => void;
}

export function usePanelCollapsed(empty: boolean, controlled?: boolean, onChange?: (collapsed: boolean) => void) {
  const [override, setOverride] = useState<boolean>();
  const collapsed = controlled ?? override ?? empty;
  const change = (next: boolean) => { setOverride(next); onChange?.(next); };
  return [collapsed, change] as const;
}

export function AowPanel({ children, className = '', headerClassName, bodyClassName, bodyRole, empty = false,
  collapsed: controlled, onCollapsedChange, controlsId, ...header }: Props) {
  const id = useId();
  const anchor = useRef<HTMLDivElement>(null);
  const navigate = useContext(PanelStackContext);
  const [collapsed, change] = usePanelCollapsed(empty, controlled, onCollapsedChange);
  useLayoutEffect(() => {
    const header = anchor.current!.firstElementChild as HTMLElement;
    const scroller = header.closest<HTMLElement>('.aow-panel-viewport');
    if (!scroller) return;
    // Fixed headers are outside the native scroll chain. Forward gestures only
    // on the header, leaving content scrolling on the browser's compositor.
    const wheel = (event: WheelEvent) => {
      if (event.defaultPrevented || event.ctrlKey || !header.dataset.docked) return;
      const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? parseFloat(getComputedStyle(scroller).lineHeight) || 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? scroller.clientHeight : 1;
      event.preventDefault();
      scroller.scrollBy({ top: event.deltaY * unit, left: event.deltaX * unit, behavior: 'instant' });
    };
    header.addEventListener('wheel', wheel, { passive: false });
    return () => header.removeEventListener('wheel', wheel);
  }, []);
  const bodyId = controlsId ?? `${id}-body`;
  return <section className={`aow-panel ${className}`} aria-label={header.title} data-collapsed={collapsed || undefined}>
    <div ref={anchor} id={`${id}-anchor`} data-panel-anchor data-panel-title={header.title}>
      <AowPanelHeader {...header} className={headerClassName} collapsed={collapsed} controlsId={bodyId}
        onToggle={() => change(!collapsed)} onNavigate={() => { if (collapsed) change(false); if (anchor.current) navigate?.(anchor.current); }} />
    </div>
    <div id={bodyId} className={`aow-panel-body ${bodyClassName ?? ''}`} role={bodyRole} hidden={collapsed}>{children}</div>
  </section>;
}

// Virtual lists in a panel share the stack's scroll position instead of adding
// an inner scrollbar. Re-measure when another panel changes height above them.
export function usePanelListViewport(count: number, rowHeight: number, active = true) {
  const host = useRef<HTMLDivElement>(null);
  const [range, setRange] = useState({ start: 0, end: 0 });
  useLayoutEffect(() => {
    const element = host.current;
    const viewport = element?.closest<HTMLElement>('.aow-panel-viewport');
    if (!element || !viewport || !active) return;
    const measure = () => {
      const box = viewport.getBoundingClientRect(), list = element.getBoundingClientRect();
      const visible = viewport.clientHeight > 0 && list.bottom > box.top && list.top < box.bottom;
      const start = visible ? Math.max(0, Math.floor((box.top - list.top) / rowHeight) - 8) : 0;
      const end = visible ? Math.min(count, Math.ceil((box.bottom - list.top) / rowHeight) + 8) : 0;
      setRange(previous => previous.start === start && previous.end === end ? previous : { start, end });
    };
    viewport.addEventListener('scroll', measure, { passive: true });
    viewport.addEventListener('aow-panel-layout', measure);
    measure();
    return () => { viewport.removeEventListener('scroll', measure); viewport.removeEventListener('aow-panel-layout', measure); };
  }, [count, rowHeight, active]);
  const reveal = useCallback((index: number) => {
    const element = host.current, viewport = element?.closest<HTMLElement>('.aow-panel-viewport');
    if (!element || !viewport) return;
    const box = viewport.getBoundingClientRect(), style = getComputedStyle(viewport);
    const top = element.getBoundingClientRect().top + index * rowHeight;
    const start = box.top + parseFloat(style.getPropertyValue('--panel-scroll-top') || '0');
    const end = box.bottom - parseFloat(style.getPropertyValue('--panel-scroll-bottom') || '0');
    if (top < start) viewport.scrollTop += top - start;
    else if (top + rowHeight > end) viewport.scrollTop += top + rowHeight - end;
  }, [rowHeight]);
  return { host, ...range, reveal };
}
