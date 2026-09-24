let editorAreaPromise: Promise<typeof import('./EditorArea')> | undefined;

export function loadEditorArea() {
  editorAreaPromise ??= import('./EditorArea');
  return editorAreaPromise;
}
