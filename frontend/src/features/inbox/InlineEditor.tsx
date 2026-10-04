import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import type { IDisposable, editor } from 'monaco-editor';
import { EditorShortcutHint } from './EditorShortcutHint';

const Editor = lazy(() => import('../editor/MonacoEditor'));

export function InlineEditor({ value, onChange, onFinish, onComposing, status, label = '需求 Markdown', seamless = false, initialHeight }: {
  value: string; onChange: (text: string) => void; onFinish: () => void; onComposing: (value: boolean) => void; status: string; label?: string;
  seamless?: boolean; initialHeight?: number;
}) {
  const host = useRef<HTMLDivElement>(null);
  const composing = useRef(false);
  const compositionCallback = useRef(onComposing); compositionCallback.current = onComposing;
  const listeners = useRef<IDisposable[]>([]);
  const [height, setHeight] = useState(seamless ? Math.max(initialHeight ?? 22, 22) : 112);
  useEffect(() => () => { listeners.current.forEach(listener => listener.dispose()); }, []);
  const mount = (instance: editor.IStandaloneCodeEditor) => {
    if (host.current) instance.updateOptions({ fontFamily: getComputedStyle(host.current).fontFamily });
    const fit = () => {
      const contentHeight = instance.getContentHeight();
      setHeight(seamless ? Math.max(initialHeight ?? 22, contentHeight) : Math.min(420, Math.max(112, contentHeight)));
    };
    const composition = (active: boolean) => { composing.current = active; compositionCallback.current(active); };
    listeners.current = [instance.onDidContentSizeChange(fit), instance.onDidLayoutChange(fit),
      instance.onDidCompositionStart(() => composition(true)), instance.onDidCompositionEnd(() => composition(false))];
    fit();
    if (!seamless) {
      const model = instance.getModel();
      if (model) instance.setPosition(model.getPositionAt(model.getValueLength()));
    }
    instance.getDomNode()?.querySelector<HTMLElement>('[role="textbox"]')?.focus({ preventScroll: seamless });
  };
  return <div className={`inbox-editor${seamless ? ' inbox-editor-seamless' : ''}`}>
    <div ref={host} className="inbox-code-editor"
      onKeyDownCapture={event => {
        if (composing.current || event.nativeEvent.isComposing) return;
        const finish = (event.metaKey || event.ctrlKey) && event.key === 'Enter';
        const escape = event.key === 'Escape' && (event.target as Element).matches('[role="textbox"]');
        if (finish || escape) { event.preventDefault(); event.stopPropagation(); onFinish(); }
      }}>
      <Suspense fallback={<div className="inbox-editor-loading" style={{ height }} role="status">正在加载编辑器…</div>}>
        <Editor language="markdown" theme="vs-dark" value={value} height={height} saveViewState={false}
          loading={<span className="inbox-editor-loading">正在加载编辑器…</span>}
          onMount={mount} onChange={text => onChange(text ?? '')}
          options={{
            ariaLabel: label, placeholder: '写下需求，支持 Markdown…', automaticLayout: true,
            fontSize: 13, lineHeight: 22, wordWrap: 'on', wrappingIndent: 'none',
            lineNumbers: 'off', lineNumbersMinChars: 0, lineDecorationsWidth: 0, glyphMargin: false, folding: false,
            minimap: { enabled: false }, overviewRulerLanes: 0, overviewRulerBorder: false, hideCursorInOverviewRuler: true,
            renderLineHighlight: 'none', scrollBeyondLastLine: false, scrollBeyondLastColumn: 0,
            padding: { top: seamless ? 0 : 10, bottom: seamless ? 0 : 10 },
            stickyScroll: { enabled: false }, guides: { indentation: false },
            occurrencesHighlight: 'off', selectionHighlight: false, renderWhitespace: 'none',
            quickSuggestions: false, wordBasedSuggestions: 'off', suggestOnTriggerCharacters: false,
            parameterHints: { enabled: false }, hover: { enabled: 'off' }, contextmenu: false, tabFocusMode: true,
            scrollbar: { vertical: seamless ? 'hidden' : 'auto', horizontal: 'hidden', verticalScrollbarSize: seamless ? 0 : 8,
              horizontalScrollbarSize: 0, alwaysConsumeMouseWheel: false, useShadows: false },
          }} />
      </Suspense>
    </div>
    {!seamless && <footer><span className="inbox-save-status" role="status">{status}</span><EditorShortcutHint /></footer>}
  </div>;
}
