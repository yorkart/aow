// Import this module dynamically so configuring Monaco stays off the initial page load.
import './monaco';
import Editor, { DiffEditor as MonacoDiffEditor, type EditorProps, type DiffEditorProps } from '@monaco-editor/react';

// Monaco 0.56 defaults to native EditContext. Keep textarea input for the
// existing keyboard/IME handlers and DOM read-only semantics across all editors.
export default function MonacoEditor({ options, ...props }: EditorProps) {
  return <Editor {...props} options={{ editContext: false, ...options }} />;
}

export function DiffEditor({ options, ...props }: DiffEditorProps) {
  return <MonacoDiffEditor {...props} options={{ editContext: false, ...options }} />;
}
