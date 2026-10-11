// Import this module dynamically so configuring Monaco stays off the initial page load.
import './monaco';
import { useEffect, useRef } from 'react';
import type { editor } from 'monaco-editor';
import Editor, { DiffEditor as MonacoDiffEditor, type EditorProps, type DiffEditorProps } from '@monaco-editor/react';

const defaultOptions = { unicodeHighlight: { ambiguousCharacters: false } };

// Use Monaco's native EditContext for editable content. Forcing the legacy
// textarea path makes IME composition across soft-wrapped lines paint its
// screen-reader buffer over the document and misplace the candidate window.
// Keep the legacy path for read-only editors to preserve domReadOnly behavior;
// unsupported browsers use Monaco's own fallback. Consumers must use Monaco's
// composition events.
export default function MonacoEditor({ options, revealLocation, onMount, ...props }: EditorProps & { revealLocation?: { line: number; revision: number } }) {
  const instance = useRef<editor.IStandaloneCodeEditor | undefined>(undefined);
  useEffect(() => {
    if (!revealLocation || !instance.current) return;
    const lineNumber = Math.max(1, Math.floor(revealLocation.line));
    instance.current.setPosition({ lineNumber, column: 1 });
    instance.current.revealLineInCenter(lineNumber);
  }, [revealLocation, props.path]);
  return <Editor {...props} onMount={(mounted, monaco) => {
    instance.current = mounted;
    if (revealLocation) { const lineNumber = Math.max(1, Math.floor(revealLocation.line)); mounted.setPosition({ lineNumber, column: 1 }); mounted.revealLineInCenter(lineNumber); }
    onMount?.(mounted, monaco);
  }} options={{ ...defaultOptions, editContext: !options?.readOnly && !options?.domReadOnly, ...options }} />;
}

export function DiffEditor({ options, ...props }: DiffEditorProps) {
  return <MonacoDiffEditor {...props} options={{ ...defaultOptions, editContext: !options?.readOnly && !options?.domReadOnly, ...options }} />;
}
