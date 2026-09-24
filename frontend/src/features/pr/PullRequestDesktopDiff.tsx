import { DiffEditor } from '@monaco-editor/react';
import { useLayoutEffect, useRef } from 'react';
import type { editor } from 'monaco-editor';
import { editorLanguage } from '../editor/language';
import '../editor/monaco';
import type { PullRequestDiff } from './types';

export default function PullRequestDesktopDiff({ diff, number, path, modelScope, sideBySide }: {
  diff: PullRequestDiff; number: number; path: string; modelScope: string; sideBySide: boolean;
}) {
  const editorRef = useRef<editor.IStandaloneDiffEditor | null>(null);
  useLayoutEffect(() => () => {
    const instance = editorRef.current;
    const models = instance?.getModel();
    // Detach before releasing either model. The React wrapper otherwise disposes
    // models first, while Monaco's diff computation is still observing them.
    instance?.setModel(null);
    models?.original.dispose();
    models?.modified.dispose();
    editorRef.current = null;
  }, []);
  const base = `inmemory://pull-request/${encodeURIComponent(modelScope)}/${number}`;
  return <DiffEditor height={420} original={diff.original!} modified={diff.modified!} language={editorLanguage(path)}
    originalModelPath={`${base}/original/${encodeURIComponent(diff.original_path ?? path)}`}
    modifiedModelPath={`${base}/modified/${encodeURIComponent(path)}`} theme="vs-dark"
    keepCurrentOriginalModel keepCurrentModifiedModel onMount={instance => { editorRef.current = instance; }}
    options={{ automaticLayout: true, readOnly: true, originalEditable: false, domReadOnly: true, minimap: { enabled: false }, fontSize: 12, renderSideBySide: sideBySide, scrollBeyondLastLine: false, padding: { top: 12 }, renderOverviewRuler: false }} />;
}
