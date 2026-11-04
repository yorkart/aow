import { t } from '../../lib/i18n';
import Editor from '../editor/MonacoEditor';

export default function TerminalInputEditor({ value, onChange, onComposing }: { value: string; onChange: (value: string) => void; onComposing: (active: boolean) => void }) {
  return <Editor language="markdown" value={value} onChange={value => onChange(value ?? '')}
    loading={<span className="terminal-input-loading">{t("aow.loading_editor")}</span>}
    onMount={editor => {
      const listeners = [editor.onDidCompositionStart(() => onComposing(true)), editor.onDidCompositionEnd(() => onComposing(false))];
      editor.onDidDispose(() => { listeners.forEach(listener => listener.dispose()); onComposing(false); });
      const model = editor.getModel();
      if (model) editor.setPosition(model.getPositionAt(model.getValueLength()));
      editor.focus();
    }}
    options={{
      ariaLabel: t("terminals.terminal_markdown_input"), automaticLayout: true, minimap: { enabled: false },
      fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
      lineHeight: 15, cursorStyle: 'line', cursorBlinking: 'blink',
      fontSize: 13, lineNumbers: 'off', glyphMargin: false, folding: false, lineDecorationsWidth: 8,
      wordWrap: 'on', scrollBeyondLastLine: false, overviewRulerLanes: 0, hideCursorInOverviewRuler: true,
      renderLineHighlight: 'none', padding: { top: 8, bottom: 8 },
      occurrencesHighlight: 'off', selectionHighlight: false,
      quickSuggestions: false, suggestOnTriggerCharacters: false, parameterHints: { enabled: false },
      contextmenu: false, tabFocusMode: true, stickyScroll: { enabled: false },
      scrollbar: { verticalScrollbarSize: 8, horizontalScrollbarSize: 8 },
    }} />;
}
