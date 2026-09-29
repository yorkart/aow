import { lazy, Suspense, useLayoutEffect, useRef, useState } from 'react';
import type { editor } from 'monaco-editor';
import { tasksApi, taskError } from './api';
import { TaskDialog } from './TaskDialog';
import { parseInboxMarkdown } from './presentation';
import type { InboxItem } from './types';

const Editor = lazy(() => import('../editor/MonacoEditor'));

export function InboxEditor({ item, projectId, onClose }: { item?: InboxItem; projectId: string; onClose: () => void }) {
  const [markdown, setMarkdown] = useState(() => item ? `${item.title}${item.description ? `\n${item.description}` : ''}` : '');
  const [busy, setBusy] = useState(false);
  const locked = useRef(false);
  const id = useRef(item?.id ?? crypto.randomUUID());
  const input = useRef<editor.IStandaloneCodeEditor | undefined>(undefined);
  const refocus = useRef(false);
  useLayoutEffect(() => { if (!busy && refocus.current) { refocus.current = false; input.current?.focus(); } }, [busy]);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const { title } = parseInboxMarkdown(markdown);
  const save = async (again: boolean) => {
    if (locked.current) return;
    const { title, description } = parseInboxMarkdown(input.current?.getValue() ?? markdown);
    if (!title) return;
    const encoder = new TextEncoder();
    if (encoder.encode(title).length > 512) { setError('标题过长，请缩短第一条非空白行。'); input.current?.focus(); return; }
    if (encoder.encode(description).length > 100000) { setError('需求正文过长，请精简后再保存。'); input.current?.focus(); return; }
    locked.current = true; setBusy(true); setError('');
    try {
      await tasksApi.inbox({ id: id.current, project_id: projectId, title, description, expected_revision: item?.revision });
      if (again) {
        id.current = crypto.randomUUID(); setMarkdown(''); input.current?.setValue('');
        setNotice('已加入 Inbox，继续录入下一条');
      } else onClose();
    } catch (error) { setError(taskError(error)); }
    finally { locked.current = false; refocus.current = true; setBusy(false); }
  };
  const mount = (instance: editor.IStandaloneCodeEditor) => {
    input.current = instance;
    const decoration = instance.createDecorationsCollection();
    const updateTitle = () => {
      const { titleLine } = parseInboxMarkdown(instance.getValue());
      const model = instance.getModel();
      decoration.set(titleLine && model ? [{
        range: { startLineNumber: titleLine, startColumn: 1, endLineNumber: titleLine, endColumn: model.getLineMaxColumn(titleLine) },
        options: { fontWeight: '700', lineHeight: 1.25, inlineClassName: 'tasks-inbox-title', inlineClassNameAffectsLetterSpacing: true },
      }] : []);
    };
    updateTitle();
    const subscription = instance.onDidChangeModelContent(updateTitle);
    instance.onDidDispose(() => { subscription.dispose(); if (input.current === instance) input.current = undefined; });
    instance.focus();
  };
  return <TaskDialog title={item ? '编辑需求' : '录入需求'} busy={busy} onClose={onClose}>
    <form onSubmit={event => { event.preventDefault(); void save(false); }}>
      <div className="tasks-inbox-editor">
        <Suspense fallback={<p className="tasks-hint">正在加载编辑器…</p>}>
          <Editor language="markdown" theme="vs-dark" value={markdown} saveViewState={false} onMount={mount}
            loading={<p className="tasks-hint">正在加载编辑器…</p>}
            onChange={value => { setMarkdown(value ?? ''); setNotice(''); }}
            options={{
              ariaLabel: '需求内容', placeholder: '随时记下一个想法…\n第一条非空白行作为标题，其余内容支持 Markdown。',
              automaticLayout: true, readOnly: busy, fontSize: 14, lineHeight: 24,
              allowVariableFonts: true, allowVariableLineHeights: true,
              minimap: { enabled: false }, lineNumbers: 'off', glyphMargin: false, folding: false, lineDecorationsWidth: 12,
              wordWrap: 'on', wrappingStrategy: 'advanced', scrollBeyondLastLine: false, overviewRulerLanes: 0, hideCursorInOverviewRuler: true,
              renderLineHighlight: 'none', padding: { top: 16, bottom: 16 }, stickyScroll: { enabled: false },
              occurrencesHighlight: 'off', selectionHighlight: false, quickSuggestions: false, wordBasedSuggestions: 'off',
              suggestOnTriggerCharacters: false, parameterHints: { enabled: false }, contextmenu: false, tabFocusMode: true,
              autoIndent: 'none', autoClosingBrackets: 'never', autoClosingQuotes: 'never', autoSurround: 'never',
              scrollbar: { verticalScrollbarSize: 8, horizontalScrollbarSize: 8 },
            }} />
        </Suspense>
      </div>
      {error && <p role="alert" className="tasks-error">{error}</p>}
      <p className="tasks-notice" role="status">{notice || '第一条非空白行作为标题 · Markdown'}</p>
      <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button type="submit" disabled={busy || !title}>{item ? '保存' : '创建'}</button>{!item && <button type="button" className="tasks-primary" disabled={busy || !title} onClick={() => void save(true)}>连续创建</button>}</footer>
    </form>
  </TaskDialog>;
}
