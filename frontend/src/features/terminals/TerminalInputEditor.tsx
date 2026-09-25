import Editor from '@monaco-editor/react';
import '../editor/monaco';

export default function TerminalInputEditor({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return <Editor language="markdown" theme="vs-dark" value={value} onChange={value => onChange(value ?? '')}
    loading={<span className="terminal-input-loading">正在加载编辑器…</span>}
    onMount={editor => {
      const model = editor.getModel();
      if (model) editor.setPosition(model.getPositionAt(model.getValueLength()));
      editor.focus();
    }}
    options={{
      // TerminalInputPane handles textarea key and composition events for submit.
      editContext: false,
      ariaLabel: '终端 Markdown 输入', automaticLayout: true, minimap: { enabled: false },
      fontFamily: "'SFMono-Regular', Consolas, 'Liberation Mono', Menlo, monospace",
      lineHeight: 15, cursorStyle: 'block', cursorBlinking: 'blink',
      fontSize: 13, lineNumbers: 'off', glyphMargin: false, folding: false, lineDecorationsWidth: 8,
      wordWrap: 'on', scrollBeyondLastLine: false, overviewRulerLanes: 0, hideCursorInOverviewRuler: true,
      renderLineHighlight: 'none', padding: { top: 8, bottom: 8 },
      occurrencesHighlight: 'off', selectionHighlight: false,
      quickSuggestions: false, suggestOnTriggerCharacters: false, parameterHints: { enabled: false },
      contextmenu: false, tabFocusMode: true, stickyScroll: { enabled: false },
      scrollbar: { verticalScrollbarSize: 8, horizontalScrollbarSize: 8 },
    }} />;
}
