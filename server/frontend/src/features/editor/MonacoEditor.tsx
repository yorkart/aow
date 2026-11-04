// Import this module dynamically so configuring Monaco stays off the initial page load.
import './monaco';
import { useEffect, useRef } from 'react';
import type { editor } from 'monaco-editor';
import Editor, { DiffEditor as MonacoDiffEditor, type EditorProps, type DiffEditorProps } from '@monaco-editor/react';
import { useTheme } from '../../lib/theme/useTheme';

const defaultOptions = { unicodeHighlight: { ambiguousCharacters: false } };

// Use Monaco's native EditContext for editable content. Forcing the legacy
// textarea path makes IME composition across soft-wrapped lines paint its
// screen-reader buffer over the document and misplace the candidate window.
// Keep the legacy path for read-only editors to preserve domReadOnly behavior;
// unsupported browsers use Monaco's own fallback. Consumers must use Monaco's
// composition events.
export default function MonacoEditor({ options, revealLocation, onMount, ...props }: EditorProps & { revealLocation?: { line: number; column?: number; length?: number; revision: number } }) {
  const theme = useTheme();
  const instance = useRef<editor.IStandaloneCodeEditor | undefined>(undefined);
  useEffect(() => {
    if (!revealLocation || !instance.current) return;
    const lineNumber = Math.max(1, Math.floor(revealLocation.line));
    const column = Math.max(1, Math.floor(revealLocation.column ?? 1));
    const range = { startLineNumber: lineNumber, startColumn: column, endLineNumber: lineNumber, endColumn: column + (revealLocation.length ?? 0) };
    instance.current.setSelection(range);
    instance.current.revealRangeInCenter(range);
    if (revealLocation.column !== undefined) instance.current.focus();
  }, [revealLocation, props.path]);
  return <Editor {...props} theme={`aow-${theme.id}`} onMount={(mounted, monaco) => {
    instance.current = mounted;
    if (revealLocation) {
      const lineNumber = Math.max(1, Math.floor(revealLocation.line)), column = Math.max(1, Math.floor(revealLocation.column ?? 1));
      const range = { startLineNumber: lineNumber, startColumn: column, endLineNumber: lineNumber, endColumn: column + (revealLocation.length ?? 0) };
      mounted.setSelection(range);
      mounted.revealRangeInCenter(range);
      if (revealLocation.column !== undefined) mounted.focus();
    }
    onMount?.(mounted, monaco);
  }} options={{ ...defaultOptions, editContext: !options?.readOnly && !options?.domReadOnly, ...options }} />;
}

export function DiffEditor({ options, ...props }: DiffEditorProps) {
  const theme = useTheme();
  return <MonacoDiffEditor {...props} theme={`aow-${theme.id}`} options={{ ...defaultOptions, editContext: !options?.readOnly && !options?.domReadOnly, ...options }} />;
}
