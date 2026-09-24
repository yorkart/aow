import { hasDocumentOwners } from './workspaceDocuments';
import Editor, { DiffEditor } from '@monaco-editor/react';
import DOMPurify from 'dompurify';
import { marked } from 'marked';
import { Check, ChevronLeft, ChevronRight, Code2, Download, Eye, LoaderCircle, RefreshCw, Save, WrapText, X } from 'lucide-react';
import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from 'react';
import { filesApi } from '../files/api';
import { useEditorSettings } from './editorSettings';
import type { MarkdownViewMode, OpenDocument } from './types';
import { monaco } from './monaco';
import { FileTypeIcon } from '../files/FileTypeIcon';

interface Props {
  documents: OpenDocument[]; activeId?: string;
  wordWrapOverride?: boolean;
  onWordWrapChange: (id: string, enabled: boolean) => void;
  onActivate: (id: string) => void; onClose: (id: string) => boolean;
  onCloseOthers: (id: string) => boolean; onCloseAll: () => boolean;
  onChange: (id: string, value: string) => void; onSave: (id: string) => void;
  onRefresh: (id: string) => void;
  onReloadExternal: (id: string) => void; onKeepLocal: (id: string) => void;
  onPreviewLoaded: (id: string, url: string) => void; onPreviewError: (id: string, url: string) => void;
  onRestorePreview: (id: string) => void;
  onMarkdownViewChange: (id: string, mode: MarkdownViewMode) => void;
}

function iconFor(document: OpenDocument) {
  return <FileTypeIcon path={document.path} className="tab-file-icon" />;
}

function fileName(path: string) {
  return path.split('/').filter(Boolean).pop() ?? path;
}

function downloadFile(path: string) {
  const link = document.createElement('a');
  link.href = filesApi.rawUrl(path);
  link.download = fileName(path);
  document.body.appendChild(link);
  link.click();
  link.remove();
}

function canSaveDocument(document: OpenDocument | undefined): document is OpenDocument {
  return Boolean(
    document
      && (document.kind === 'text' || document.kind === 'json' || document.kind === 'markdown')
      && !document.readOnly
      && document.dirty
      && !document.saving
      && !document.refreshing
      && !document.pendingExternal,
  );
}

export function documentEditorModelPath(document: OpenDocument) {
  if (document.instanceId === undefined) return document.path;
  return `inmemory://aow/${encodeURIComponent(document.id)}/${document.instanceId}/${encodeURIComponent(fileName(document.path))}`;
}

function diffModelPath(document: OpenDocument, side: 'original' | 'modified' | 'fallback') {
  return `inmemory://git-diff/${encodeURIComponent(document.id)}/${side}/${encodeURIComponent(fileName(document.path))}`;
}

function closeDocument(document: OpenDocument, onClose: Props['onClose']) {
  if (!onClose(document.id)) return;
  disposeEditorModels([document]);
}

export function disposeEditorModels(documents: OpenDocument[]) {
  const paths = documents.filter(document => !hasDocumentOwners(document)).flatMap((document) => document.id.startsWith('diff:') || document.id.startsWith('commit-diff:')
    ? (['original', 'modified', 'fallback'] as const).map((side) => diffModelPath(document, side))
    : [documentEditorModelPath(document)]);
  // Capture the instances now: reopening the same path before this timer runs
  // must not dispose a newly created model. React detaches the closing editor first.
  const models = paths.flatMap((path) => monaco.editor.getModel(monaco.Uri.parse(path)) ?? []);
  if (!models.length) return;
  window.setTimeout(() => {
    for (const model of models) if (!model.isDisposed()) model.dispose();
  }, 0);
}

interface TabContextMenu { x: number; y: number; documentId: string }

interface TabContextMenuProps {
  x: number;
  y: number;
  document: OpenDocument;
  hasOtherDocuments: boolean;
  onCloseMenu: () => void;
  onClose: () => void;
  onCloseOthers: () => void;
  onCloseAll: () => void;
}

function TabContextMenu({ x, y, document, hasOtherDocuments, onCloseMenu, onClose, onCloseOthers, onCloseAll }: TabContextMenuProps) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - bounds.height - 4)),
    });
  }, [x, y]);

  useEffect(() => {
    const close = () => onCloseMenu();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onCloseMenu(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    window.addEventListener('keydown', key);
    return () => {
      window.removeEventListener('pointerdown', close);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
      window.removeEventListener('keydown', key);
    };
  }, [onCloseMenu]);

  const action = (callback: () => void) => (event: ReactMouseEvent) => {
    event.stopPropagation();
    onCloseMenu();
    callback();
  };

  return (
    <div ref={menu} className="editor-tab-context-menu" style={{ left: position.x, top: position.y }} role="menu" aria-label={`${document.name} 标签操作`} onPointerDown={(event) => event.stopPropagation()}>
      <button className="danger" role="menuitem" onClick={action(onClose)}>关闭</button>
      <button className="danger" role="menuitem" disabled={!hasOtherDocuments} onClick={action(onCloseOthers)}>关闭其他</button>
      <button className="danger" role="menuitem" onClick={action(onCloseAll)}>关闭所有</button>
    </div>
  );
}

export function EditorArea({ documents, activeId, wordWrapOverride, onWordWrapChange, onActivate, onClose, onCloseOthers, onCloseAll, onChange, onSave, onRefresh, onReloadExternal, onKeepLocal, onPreviewLoaded, onPreviewError, onRestorePreview, onMarkdownViewChange }: Props) {
  const active = documents.find((document) => document.id === activeId);
  const { editor } = useEditorSettings();
  const wrapEnabled = wordWrapOverride ?? editor.word_wrap;
  const wordWrap = wrapEnabled ? 'on' : 'off';
  const saveLabel = active?.saving ? '自动保存中…' : active?.dirty ? '立即保存' : '已自动保存';
  const editorArea = useRef<HTMLElement>(null);
  const tabs = useRef<HTMLDivElement>(null);
  const [contextMenu, setContextMenu] = useState<TabContextMenu>();
  const scrollTabs = (distance: number) => tabs.current?.scrollBy({ left: distance, behavior: 'smooth' });
  const previousDocument = useRef<OpenDocument | undefined>(undefined);
  const restoreRefreshView = useRef<(() => void) | undefined>(undefined);

  // Snapshot immediately before the React Monaco wrapper applies new values
  // in its effects, so scrolling or typing during the request is respected.
  useLayoutEffect(() => {
    const previous = previousDocument.current;
    previousDocument.current = active;
    restoreRefreshView.current = undefined;
    if (!active || active.refreshing || !previous?.refreshing
      || previous.id !== active.id || previous.instanceId !== active.instanceId) return;
    const views = monaco.editor.getEditors().flatMap((editor) => {
      const node = editor.getDomNode();
      return node && editorArea.current?.contains(node)
        ? [{ editor, node, model: editor.getModel(), state: editor.saveViewState() }] : [];
    });
    restoreRefreshView.current = () => {
      for (const { editor, node, model, state } of views) {
        if (state && node.isConnected && editor.getModel() === model) editor.restoreViewState(state);
      }
    };
  }, [active]);
  useEffect(() => {
    restoreRefreshView.current?.();
    restoreRefreshView.current = undefined;
  }, [active]);

  useEffect(() => {
    const saveWithShortcut = (event: KeyboardEvent) => {
      const isMacOS = navigator.userAgent.includes('Macintosh');
      const saveShortcutPressed = isMacOS ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey;
      if (
        event.repeat
        || event.shiftKey
        || event.altKey
        || !saveShortcutPressed
        || event.key.toLowerCase() !== 's'
        || !editorArea.current?.contains(event.target as Node)
        || !canSaveDocument(active)
      ) return;

      event.preventDefault();
      onSave(active.id);
    };

    window.addEventListener('keydown', saveWithShortcut, true);
    return () => window.removeEventListener('keydown', saveWithShortcut, true);
  }, [active, onSave]);

  const fileNameCounts = new Map<string, number>();
  for (const document of documents) {
    const name = fileName(document.path);
    fileNameCounts.set(name, (fileNameCounts.get(name) ?? 0) + 1);
  }
  return (
    <main ref={editorArea} className="editor-area">
      <div className="tabs-shell">
        <button className="tabs-scroll-button" title="向左滚动" onClick={() => scrollTabs(-260)}><ChevronLeft size={15} /></button>
        <div
          ref={tabs}
          className="editor-tabs"
          onWheel={(event) => {
            if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) {
              tabs.current?.scrollBy({ left: event.deltaY });
            }
          }}
        >
          {documents.map((document) => {
            const name = fileName(document.path);
            const title = fileNameCounts.get(name)! > 1 ? document.path : name;
            return (
              <button
                key={document.id}
                className={document.id === activeId ? 'active' : ''}
                onClick={() => onActivate(document.id)}
                onContextMenu={(event) => {
                  event.preventDefault();
                  onActivate(document.id);
                  setContextMenu({ x: event.clientX, y: event.clientY, documentId: document.id });
                }}
                title={document.path}
              >
                {iconFor(document)}<span>{title}{document.dirty ? ' ●' : ''}</span>
                <X size={13} onClick={(event) => { event.stopPropagation(); closeDocument(document, onClose); }} />
              </button>
            );
          })}
        </div>
        <button className="tabs-scroll-button" title="向右滚动" onClick={() => scrollTabs(260)}><ChevronRight size={15} /></button>
      </div>
      {contextMenu ? (() => {
        const menuDocument = documents.find((document) => document.id === contextMenu.documentId);
        if (!menuDocument) return null;
        const otherDocuments = documents.filter((document) => document.id !== menuDocument.id);
        return (
          <TabContextMenu
            x={contextMenu.x}
            y={contextMenu.y}
            document={menuDocument}
            hasOtherDocuments={otherDocuments.length > 0}
            onCloseMenu={() => setContextMenu(undefined)}
            onClose={() => closeDocument(menuDocument, onClose)}
            onCloseOthers={() => {
              if (onCloseOthers(menuDocument.id)) disposeEditorModels(otherDocuments);
            }}
            onCloseAll={() => {
              if (onCloseAll()) disposeEditorModels(documents);
            }}
          />
        );
      })() : null}
      {!active ? <div className="welcome"><h1>AOW</h1><p>从左侧文件树选择文件，或切换到 Source Control 查看更改。</p></div> : (
        <>
          <div className="editor-toolbar">
            <code title={active.path}>{active.path}</code>
            {active.kind === 'markdown' ? (
              <div className="markdown-view-switch" role="group" aria-label="Markdown 查看方式">
                <button className={active.markdownView !== 'editor' ? 'active' : ''} title="预览" aria-label="预览" aria-pressed={active.markdownView !== 'editor'} onClick={() => onMarkdownViewChange(active.id, 'preview')}><Eye aria-hidden="true" /></button>
                <button className={active.markdownView === 'editor' ? 'active' : ''} title="编辑" aria-label="编辑" aria-pressed={active.markdownView === 'editor'} onClick={() => onMarkdownViewChange(active.id, 'editor')}><Code2 aria-hidden="true" /></button>
              </div>
            ) : null}
            {(active.kind === 'text' || active.kind === 'json' || active.kind === 'markdown') && !active.readOnly ? <button
              title={`${saveLabel}；自动保存已开启（macOS: Command+S；其他系统: Ctrl+S）`} aria-label={saveLabel} aria-busy={Boolean(active.saving)}
              disabled={!canSaveDocument(active)} onClick={() => onSave(active.id)}>
              {active.saving ? <LoaderCircle className="spinning" aria-hidden="true" /> : active.dirty ? <Save aria-hidden="true" /> : <Check aria-hidden="true" />}
            </button> : null}
            <button title={active.refreshing ? '正在刷新…' : active.diffSource ? '刷新 Diff' : '刷新文件'} aria-label={active.diffSource ? '刷新 Diff' : '刷新文件'} aria-busy={Boolean(active.refreshing)} disabled={active.loading || active.refreshing || active.saving} onClick={() => onRefresh(active.id)}><RefreshCw className={active.refreshing ? 'spinning' : undefined} aria-hidden="true" /></button>
            {!active.diffSource ? <button title="下载文件" aria-label="下载文件" onClick={() => downloadFile(active.path)}><Download aria-hidden="true" /></button> : null}
            {active.kind === 'text' || active.kind === 'json' || active.kind === 'diff' || (active.kind === 'markdown' && active.markdownView === 'editor') ? <button
              className={wrapEnabled ? 'active' : undefined} aria-label="自动换行" aria-pressed={wrapEnabled}
              title={`自动换行：${wrapEnabled ? '已开启' : '已关闭'}（${wordWrapOverride === undefined ? '跟随全局设置' : '当前 Tab'}）`}
              onClick={() => onWordWrapChange(active.id, !wrapEnabled)}><WrapText aria-hidden="true" /></button> : null}
          </div>
          {active.pendingExternal ? (
            <div className="editor-file-notice warning">
              <span>文件已在磁盘上更新。请选择重新加载，或用当前编辑内容覆盖磁盘版本。</span>
              <button disabled={active.refreshing} onClick={() => onReloadExternal(active.id)}>从磁盘重新加载</button>
              <button disabled={active.refreshing} onClick={() => onKeepLocal(active.id)}>保留并覆盖</button>
            </div>
          ) : null}
          {active.kind === 'pdf' && active.previewHistory?.length ? (
            <div className="editor-file-notice info">
              <span>PDF 已刷新。如果浏览器无法显示新版本，可以恢复上一个预览。</span>
              <button onClick={() => onRestorePreview(active.id)}>恢复上一个预览</button>
            </div>
          ) : null}
          {active.refreshError ? <div className="editor-file-notice error"><span>刷新失败：{active.refreshError}</span></div> : null}
          <div className="editor-content">
            {active.loading ? <div className="editor-loading"><span className="loading-spinner" />加载中…</div> : null}
            {!active.loading && (active.kind === 'text' || active.kind === 'json') ? (
              <Editor
                path={documentEditorModelPath(active)}
                keepCurrentModel
                language={active.language}
                value={active.content ?? ''}
                theme="vs-dark"
                options={{ automaticLayout: true, minimap: { enabled: false }, readOnly: active.readOnly, fontSize: 13, scrollBeyondLastLine: false, wordWrap }}
                onChange={(value) => onChange(active.id, value ?? '')}
              />
            ) : null}
            {!active.loading && active.kind === 'markdown' ? (
              <>
                <div className={`markdown-source-editor${active.markdownView === 'editor' ? '' : ' preview-hidden'}`} aria-hidden={active.markdownView !== 'editor'}>
                  <Editor
                    path={documentEditorModelPath(active)}
                    language={active.language ?? 'markdown'}
                    value={active.content ?? ''}
                    theme="vs-dark"
                    keepCurrentModel
                    options={{ automaticLayout: true, minimap: { enabled: false }, readOnly: active.readOnly, fontSize: 13, scrollBeyondLastLine: false, wordWrap }}
                    onChange={(value) => onChange(active.id, value ?? '')}
                  />
                </div>
                {active.markdownView !== 'editor' ? <article className="markdown-preview" dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(marked.parse(active.content ?? '') as string) }} /> : null}
              </>
            ) : null}
            {!active.loading && active.kind === 'docx' ? <article className="markdown-preview docx-preview" dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(active.content ?? '') }} /> : null}
            {!active.loading && active.kind === 'image' && active.imageUrl ? <div className="image-preview"><img key={active.imageUrl} src={active.imageUrl} alt={active.name} onLoad={(event) => onPreviewLoaded(active.id, event.currentTarget.currentSrc || event.currentTarget.src)} onError={(event) => onPreviewError(active.id, event.currentTarget.currentSrc || event.currentTarget.src)} /></div> : null}
            {!active.loading && active.kind === 'pdf' && active.imageUrl ? <iframe key={active.imageUrl} className="pdf-preview" src={active.imageUrl} title={active.name} onError={(event) => onPreviewError(active.id, event.currentTarget.src)} /> : null}
            {!active.loading && active.kind === 'diff' && active.originalContent !== undefined ? (
              // The React wrapper switches each inner editor's model separately,
              // leaving Monaco's diff model bound to the previous pair. Remount
              // per document so the displayed models and computed diff stay aligned.
              <DiffEditor
                key={active.id}
                original={active.originalContent}
                modified={active.content ?? ''}
                language={active.language ?? 'plaintext'}
                originalModelPath={diffModelPath(active, 'original')}
                modifiedModelPath={diffModelPath(active, 'modified')}
                theme="vs-dark"
                keepCurrentOriginalModel
                keepCurrentModifiedModel
                options={{
                  automaticLayout: true, readOnly: true, minimap: { enabled: false }, fontSize: 12,
                  // Monaco's automatic inline mode leaves the original editor's
                  // wrap override disabled when returning to side-by-side mode.
                  renderSideBySide: true, useInlineViewWhenSpaceIsLimited: false,
                  // Monaco always reserves this margin on the original side.
                  glyphMargin: true, wordWrap, diffWordWrap: wordWrap,
                }}
              />
            ) : null}
            {!active.loading && active.kind === 'diff' && active.originalContent === undefined ? (
              <Editor
                path={diffModelPath(active, 'fallback')}
                keepCurrentModel
                language={active.language ?? 'plaintext'}
                value={active.content ?? ''}
                theme="vs-dark"
                options={{ automaticLayout: true, minimap: { enabled: false }, readOnly: true, fontSize: 12, scrollBeyondLastLine: false, wordWrap }}
              />
            ) : null}
            {active.kind === 'unsupported' ? <div className="unsupported-preview"><h2>无法直接预览</h2><p>{active.error}</p></div> : null}
          </div>
        </>
      )}
    </main>
  );
}
