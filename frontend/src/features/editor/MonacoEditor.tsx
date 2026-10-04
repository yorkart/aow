// Import this module dynamically so configuring Monaco stays off the initial page load.
import './monaco';
import Editor, { DiffEditor as MonacoDiffEditor, type EditorProps, type DiffEditorProps } from '@monaco-editor/react';

const defaultOptions = { unicodeHighlight: { ambiguousCharacters: false } };

// Use Monaco's native EditContext for editable content. Forcing the legacy
// textarea path makes IME composition across soft-wrapped lines paint its
// screen-reader buffer over the document and misplace the candidate window.
// Keep the legacy path for read-only editors to preserve domReadOnly behavior;
// unsupported browsers use Monaco's own fallback. Consumers must use Monaco's
// composition events.
export default function MonacoEditor({ options, ...props }: EditorProps) {
  return <Editor {...props} options={{ ...defaultOptions, editContext: !options?.readOnly && !options?.domReadOnly, ...options }} />;
}

export function DiffEditor({ options, ...props }: DiffEditorProps) {
  return <MonacoDiffEditor {...props} options={{ ...defaultOptions, editContext: !options?.readOnly && !options?.domReadOnly, ...options }} />;
}
