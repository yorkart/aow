import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState, type ComponentProps } from 'react';
import { ChevronUp } from 'lucide-react';
import type { TerminalPaneControls } from './terminalPresentation';
import type { TerminalConnectionState } from './terminalState';
import { TerminalPaneView } from './TerminalPaneView';
import './terminal-input.css';

const TerminalInputEditor = lazy(() => import('./TerminalInputEditor'));
const DEFAULT_INPUT_HEIGHT = 80;

type Props = Pick<ComponentProps<typeof TerminalPaneView>, 'visible' | 'tabId' | 'pane' | 'active' | 'onFocus' | 'onStatus'>;

export function TerminalInputPane(props: Props) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState('');
  const [height, setHeight] = useState(DEFAULT_INPUT_HEIGHT);
  const [availableHeight, setAvailableHeight] = useState(0);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState('');
  const [connection, setConnection] = useState<TerminalConnectionState>('connecting');
  const controls = useRef<TerminalPaneControls | null>(null);
  const container = useRef<HTMLDivElement>(null);
  const composer = useRef<HTMLDivElement>(null);
  const composing = useRef(false);
  const drag = useRef<{ y: number; height: number } | null>(null);
  const ready = props.visible && props.pane.status === 'running' && connection === 'connected';
  const shown = open && ready && props.active;
  const maxHeight = Math.max(0, availableHeight - 40);
  const minHeight = Math.min(DEFAULT_INPUT_HEIGHT, maxHeight);
  const editorHeight = Math.max(minHeight, Math.min(height, maxHeight));

  const dismiss = () => {
    setOpen(false);
    setDragging(false);
    drag.current = null;
    composing.current = false;
  };

  useLayoutEffect(() => {
    const node = container.current;
    if (!node) return;
    const measure = () => setAvailableHeight(node.clientHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    if (shown) composer.current?.focus({ preventScroll: true });
  }, [shown]);

  useEffect(() => { if (!ready || !props.active) dismiss(); }, [ready, props.active]);

  useEffect(() => {
    if (!shown) return;
    const outside = (event: PointerEvent) => {
      if (!composer.current?.contains(event.target as Node)) dismiss();
    };
    const hidden = () => { if (document.hidden) dismiss(); };
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('visibilitychange', hidden);
    window.addEventListener('blur', dismiss);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('visibilitychange', hidden);
      window.removeEventListener('blur', dismiss);
    };
  }, [shown]);

  const submit = () => {
    if (!draft.trim() || composing.current) return;
    try {
      if (!controls.current?.submit(draft)) {
        setError('终端暂时无法接收输入，草稿已保留');
        return;
      }
      setDraft('');
      setError('');
      dismiss();
      controls.current.focus();
    } catch {
      setError('发送失败，草稿已保留');
    }
  };

  return <div ref={container} className={`terminal-input-pane${dragging ? ' resizing' : ''}`}>
    {/* Transforms leave the measured xterm container and PTY grid unchanged. */}
    <div className="terminal-input-surface" style={{ transform: `translateY(-${shown ? editorHeight : 0}px)` }}>
      <TerminalPaneView {...props} controlsRef={controls} inputSuspended={shown} onConnectionChange={setConnection} />
    </div>
    {ready && !shown ? <button type="button" className="terminal-input-trigger" aria-label="打开终端输入编辑器"
      onClick={() => { props.onFocus(); setError(''); setOpen(true); }}>
      <span><ChevronUp aria-hidden="true" />点击编辑输入</span>
    </button> : null}
    {shown ? <div ref={composer} className="terminal-input-composer" style={{ height: editorHeight }}
      role="group" aria-label="终端输入编辑器" tabIndex={-1}
      onBlurCapture={event => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) dismiss();
      }}
      onCompositionStartCapture={() => { composing.current = true; }}
      onCompositionEndCapture={() => { composing.current = false; }}
      onKeyDownCapture={event => {
        if (event.nativeEvent.isComposing || composing.current || event.keyCode === 229) return;
        if (event.key === 'Enter' && event.shiftKey && event.target instanceof HTMLTextAreaElement
          && event.target.closest('.monaco-editor')) {
          event.preventDefault();
          event.stopPropagation();
          if (!event.repeat) submit();
        } else if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          dismiss();
          controls.current?.focus();
        }
      }}>
      <div className="terminal-input-resize" role="separator" aria-label="调整输入编辑器高度"
        aria-orientation="horizontal" tabIndex={0} aria-valuenow={Math.round(editorHeight)}
        aria-valuemin={minHeight} aria-valuemax={maxHeight}
        onKeyDown={event => {
          if (event.key !== 'ArrowUp' && event.key !== 'ArrowDown') return;
          event.preventDefault();
          event.stopPropagation();
          setHeight(Math.max(minHeight, Math.min(maxHeight, editorHeight + (event.key === 'ArrowUp' ? 20 : -20))));
        }}
        onPointerDown={event => {
          if (event.button !== 0) return;
          event.preventDefault();
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = { y: event.clientY, height: editorHeight };
          setDragging(true);
        }}
        onPointerMove={event => {
          if (!drag.current) return;
          setHeight(Math.max(minHeight, Math.min(maxHeight, drag.current.height + drag.current.y - event.clientY)));
        }}
        onPointerUp={event => {
          event.currentTarget.releasePointerCapture(event.pointerId);
          drag.current = null;
          setDragging(false);
        }}
        onLostPointerCapture={() => { drag.current = null; setDragging(false); }} />
      <div className="terminal-input-editor">
        <Suspense fallback={<span className="terminal-input-loading">正在加载编辑器…</span>}>
          <TerminalInputEditor value={draft} onChange={setDraft} />
        </Suspense>
      </div>
      {error ? <div className="terminal-input-error" role="alert">{error}</div> : null}
    </div> : null}
  </div>;
}
