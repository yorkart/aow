import { ReviewProviderSettings } from '../features/pr/ReviewProviderSettings';
import { ConfigurationSettings } from '../features/configuration/ConfigurationSettings';
import { appLocalStorage } from '../lib/basePath';
import { useWorkspaceDocuments, updateSharedDocument, nextDocumentInstanceId, sharedDocument, isPreviewOwned, savingDocuments, failedDocuments } from '../features/editor/workspaceDocuments';
import { Fragment, lazy, memo, Suspense, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { FloatingWorkspace } from './FloatingWorkspace';
import { useAowTabLocation, useAowTabLocationError } from './AowTabEntry';
import { captureTabTarget, openTabTarget, tabCenterId, type ResolvedTab, type TabOpenActions, type AutomationLocation } from './tabRoutes';
import { diffDocument } from './tabRoutes/diff';
import { prTabId } from './tabRoutes/pr';
import { FloatingWorkspaceProvider, FloatingOpenMenu, useFloatingWorkspace, openingInFloatingWorkspace, withFloatingOpen, readStored, persist } from './floatingWorkspaceState';
import type { CSSProperties, DragEvent as ReactDragEvent, PointerEvent as ReactPointerEvent } from 'react';
import {
  ArrowUp, Bell, Bot, CalendarClock, Check, ChevronDown, ChevronRight, CircleHelp, CornerDownLeft, FileText, Files, FolderGit2, FolderOpen, GitBranch, GitBranchPlus, GitPullRequest, MessageSquare, MoreHorizontal,
  LoaderCircle, Network, NotebookPen, PanelLeftClose, PanelLeftOpen, PanelRightClose, PanelRightOpen, Pencil, Pin, PinOff, Plus, RefreshCw, Settings, SquareTerminal, Trash2, X,
} from 'lucide-react';
import { gitApi } from '../features/git/api';
import { filesApi } from '../features/files/api';
import { defaultEditorSettings, EditorSettingsProvider, useEditorSettings, useWordWrapOverrides } from '../features/editor/editorSettings';
import { AutomationDetail } from '../features/automations/AutomationDetail';
import { AutomationPanel } from '../features/automations/AutomationPanel';
import type { AutomationRun, AutomationTask } from '../features/automations/types';
import { TerminalPanel } from '../features/terminals/TerminalPanel';
import type { TerminalSort } from '../features/terminals/TerminalScopeMenu';
import { isCliTerminal, terminalTabPresentation } from '../features/terminals/terminalPresentation';
import { AgentSessions } from '../features/sessions/AgentSessions';
import { AgentIcon } from '../features/agents/AgentIcon';
import { agentTypes, builtinAgentType, aowAgentType } from '../features/agents/agentTypes';
import { NotificationSettingsPanel } from '../features/notifications/NotificationSettingsPanel';
import { SessionShareButton } from '../features/sessions/SessionShareButton';
import { Explorer } from '../features/files/Explorer';
import { SystemFileBrowser } from '../features/files/SystemFileBrowser';
import { DirectoryTypeIcon } from '../features/files/FileTypeIcon';
import { PullRequestDetailView } from '../features/pr/PullRequestDetailView';
import { PullRequestsPanel } from '../features/pr/PullRequestsPanel';
import { SessionSnapshotView } from '../features/sessions/SessionSnapshotView';
import { SourceControl } from '../features/git/SourceControl';
import { TerminalWorkspace } from '../features/terminals/TerminalWorkspace';
import { AowIconButton } from '../components/AowIconButton';
import { useConfirmation } from '../components/ConfirmationDialog';
import { AowPanel, AowPanelStack } from '../components/AowPanel';
import { AowNodeSwitcher } from './AowNodeSwitcher';
import { parseNodeAddresses } from './aowNodes';
import { WorktreeIcon, worktreeColors, worktreeColorValues, worktreeIconGroups, worktreeIconLabel } from './WorktreeIcon';
import { WorkspaceTabs, type WorkspaceTab as CenterTab, type TabRevealRequest, workspaceTabGroup as centerTabGroup, groupWorkspaceTabs } from './WorkspaceTabs';
import { loadEditorArea } from '../features/editor/editorLoader';
import { editorLanguage, extension } from '../features/editor/language';
import { terminalApi } from '../features/terminals/terminalApi';
import type { AgentSessionSnapshot, AowAgentSession } from '../features/sessions/types';
import type { FileEntry } from '../features/files/types';
import type { GitCommit, GitCommitFile, GitFileStatus, RepositorySummary } from '../features/git/types';
import type { MarkdownViewMode, OpenDocument, PreviewKind } from '../features/editor/types';
import type { PullRequestSummary } from '../features/pr/types';
import type { TerminalTab } from '../features/terminals/types';
import type { AowAgent } from '../features/agents/types';
import type { AowProject, AowWorktree, AowSettings, WorktreeColor, WorktreeIconId } from './types';
import { useTerminals } from '../features/terminals/useTerminals';
import { useProjectTerminals } from '../features/terminals/useProjectTerminals';
import { usePinnedWorktrees } from './usePinnedWorktrees';
import { useWorktreeResources } from './useWorktreeResources';
import { removalActive, useWorktreeRemovals } from './useWorktreeRemovals';
import { WorktreeCleanupDialog } from './WorktreeCleanupDialog';
import { useOperations } from '../features/operations/operations';
import { OperationStatus } from '../features/operations/OperationStatus';
import { OperationLogPanel } from '../features/operations/OperationLogPanel';
import { useWorktreeUnreadCounts } from '../features/notifications/taskNotifications';
import type { WorktreeResourceState } from './worktreeResources';
import { aowApi } from './aowApi';
import { ProjectIcon } from './ProjectIcon';
import { agentsApi } from '../features/agents/api';
import { sessionsApi } from '../features/sessions/api';
import type { WorktreeRemovalJob, WorktreeRemovalPreview } from './types';

const EditorArea = lazy(() => loadEditorArea().then((module) => ({ default: module.EditorArea })));
const imageExtensions = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif']);
const textExtensions = new Set(['rs', 'ts', 'tsx', 'js', 'jsx', 'py', 'go', 'java', 'c', 'cc', 'cpp', 'h', 'hpp', 'css', 'html', 'xml', 'toml', 'yaml', 'yml', 'sh', 'bash', 'txt', 'log', 'ini', 'conf']);
const leftSidebarWidthStorageKey = 'aow-left-width';
const leftSidebarVisibleStorageKey = 'aow-left-visible';
const rightSidebarWidthStorageKey = 'aow-right-width';
const rightSidebarVisibleStorageKey = 'aow-right-visible';
const defaultLeftSidebarWidth = 260;
const defaultRightSidebarWidth = 340;
const minLeftSidebarWidth = 180;
const maxLeftSidebarWidth = 520;
const minRightSidebarWidth = 220;
const maxRightSidebarWidth = 640;
const minCenterWidth = 360;

type RightView = 'files' | 'git' | 'pullRequests' | 'sessions' | 'automations' | 'terminals';
type AutomationSessionReference = { taskId: string; runId: string };
type SessionPreview = { session: AowAgentSession; workspacePath: string; snapshot?: AgentSessionSnapshot; loading: boolean; error?: string; automationRun?: AutomationSessionReference };
type WorktreeEntry = { project: AowProject; worktree: AowWorktree };
type WorktreeContextMenuState = WorktreeEntry & { x: number; y: number };
type ProjectMenuState = { project: AowProject; x: number; y: number };
type RemoveWorktreeState = WorktreeEntry & { preview: WorktreeRemovalPreview };
type PinnedDropTarget = { path: string; position: 'before' | 'after' };
type ExplorerRefresh = { generation: number; directory: string };

const initialExplorerRefresh: ExplorerRefresh = { generation: 0, directory: '' };
const autoSaveDelayMs = 750;

function message(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

function basename(path: string) {
  return path.split('/').filter(Boolean).pop() ?? path;
}

function dirname(path: string) {
  const normalized = path.replace(/\/+$/, '');
  const index = normalized.lastIndexOf('/');
  return index <= 0 ? '/' : normalized.slice(0, index);
}

function joinPath(parent: string, child: string) {
  return parent === '/' ? `/${child}` : `${parent}/${child}`;
}

function worktreeDirectoryName(project: AowProject, branch: string) {
  const suffix = branch.trim().replace(/[^a-zA-Z0-9._-]+/g, '-').replace(/^-+|-+$/g, '') || 'worktree';
  return `${basename(project.registered_path)}-${suffix}`;
}

function suggestedWorktreePath(project: AowProject, branch: string) {
  const mainWorktreePath = project.worktrees.find((worktree) => worktree.is_main)?.path
    ?? project.registered_path;
  return joinPath(dirname(mainWorktreePath), worktreeDirectoryName(project, branch));
}

function shellQuote(value: string) {
  return `'${value.split("'").join("'\"'\"'")}'`;
}

function storedWidth(key: string, fallback: number, min: number, max: number) {
  const value = Number(appLocalStorage.getItem(key));
  return Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}

function revokeBlobUrl(url?: string) {
  if (url?.startsWith('blob:')) window.setTimeout(() => { if (!isPreviewOwned(url)) URL.revokeObjectURL(url); }, 0);
}

async function readDocumentDiff(document: OpenDocument, signal?: AbortSignal) {
  const source = document.diffSource!;
  const diff = source.kind === 'working'
    ? await gitApi.gitDiff(source.repository, document.path, source.staged, signal)
    : await gitApi.gitCommitDiff(source.repository, source.commit, document.path, source.originalPath, signal);
  let content = diff.modified ?? diff.patch;
  if (source.kind === 'working' && source.untracked && !diff.patch && diff.modified == null) {
    content = (await filesApi.readText(`${source.repository}/${document.path}`, signal)).content;
  }
  return { kind: 'diff' as const, content, originalContent: diff.original ?? undefined, language: editorLanguage(document.path) };
}

function canAutoSaveDocument(document: OpenDocument): document is OpenDocument & { content: string } {
  return (document.kind === 'text' || document.kind === 'json' || document.kind === 'markdown')
    && document.content !== undefined
    && Boolean(document.dirty)
    && !document.readOnly
    && !document.saving
    && !document.refreshing
    && !document.pendingExternal;
}

function updateWorktreeAppearance(
  projects: AowProject[],
  path: string,
  appearance: Partial<Pick<AowWorktree, 'color' | 'icon'>>,
) {
  return projects.map((project) => {
    if (!project.worktrees.some((worktree) => worktree.path === path)) return project;
    return {
      ...project,
      worktrees: project.worktrees.map((worktree) => worktree.path === path
        ? { ...worktree, ...appearance }
        : worktree),
    };
  });
}

function previewKind(path: string): PreviewKind {
  const ext = extension(path);
  if (imageExtensions.has(ext)) return 'image';
  if (ext === 'md' || ext === 'markdown') return 'markdown';
  if (ext === 'json') return 'json';
  if (ext === 'pdf') return 'pdf';
  if (ext === 'docx' || ext === 'doc') return 'unsupported';
  if (textExtensions.has(ext) || !ext) return 'text';
  return 'text';
}

function terminalCenterTab(tab: TerminalTab, detected: Record<string, string | null>, titles: Record<string, string>): CenterTab {
  const agent = tab.panes.find((pane) => pane.kind === 'agent');
  const terminal = terminalTabPresentation(tab, detected, titles);
  const cli = isCliTerminal(tab);
  return { id: `terminal:${tab.id}`, kind: agent ? 'agent' : 'terminal', label: terminal.title, targetId: tab.id, terminal, renameable: !cli };
}

function WorktreeContextMenu({
  state, pinned, savingAppearance, onClose, onTogglePin, onColorChange, onIconChange, onRemove,
}: {
  state: WorktreeContextMenuState;
  pinned: boolean;
  savingAppearance: boolean;
  onClose: () => void;
  onTogglePin: () => void;
  onColorChange: (color: WorktreeColor) => void;
  onIconChange: (icon: WorktreeIconId) => void;
  onRemove: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x: state.x, y: state.y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(state.x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(state.y, window.innerHeight - bounds.height - 4)),
    });
  }, [state.x, state.y]);

  useEffect(() => {
    const close = () => onClose();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
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
  }, [onClose]);

  return <div ref={menu} className="project-aow-context-menu project-aow-worktree-menu" style={{ left: position.x, top: position.y }} role="menu" aria-label={`${state.worktree.path} 操作`} onPointerDown={(event) => event.stopPropagation()}>
    <div className="project-aow-context-path" title={state.worktree.path}>{state.project.name} · {state.worktree.branch || basename(state.worktree.path)}</div>
    <button role="menuitem" onClick={() => { onTogglePin(); onClose(); }}>{pinned ? <PinOff /> : <Pin />}{pinned ? 'Unpin' : 'Pin to top'}</button>
    <div className="project-aow-worktree-icon-picker" role="group" aria-label="Worktree 图标">
      <div className="project-aow-worktree-icon-label">图标</div>
      <button className="project-aow-worktree-icon-default" role="menuitemradio" aria-checked={(state.worktree.icon ?? 'default') === 'default'} disabled={savingAppearance} onClick={() => { onIconChange('default'); onClose(); }}>
        <WorktreeIcon style={{ color: worktreeColorValues[state.worktree.color ?? 'default'] }} />默认图标{(state.worktree.icon ?? 'default') === 'default' ? <Check /> : null}
      </button>
      {worktreeIconGroups.map((group) => <div key={group.label} className="project-aow-worktree-icons" role="group" aria-label={group.label}>
        {group.icons.map((icon) => <button key={icon} className={`project-aow-worktree-icon${state.worktree.icon === icon ? ' selected' : ''}`}
          type="button" role="menuitemradio" aria-checked={state.worktree.icon === icon} aria-label={worktreeIconLabel(icon)} title={worktreeIconLabel(icon)} disabled={savingAppearance}
          onClick={() => { onIconChange(icon); onClose(); }}>
          <WorktreeIcon icon={icon} style={{ color: worktreeColorValues[state.worktree.color ?? 'default'] }} />
        </button>)}
      </div>)}
    </div>
    <div className="project-aow-worktree-color-picker">
      <div className="project-aow-worktree-color-label">图标颜色</div>
      <div className="project-aow-worktree-colors" role="group" aria-label="Worktree 图标颜色">
        {worktreeColors.map((color) => {
          const selected = (state.worktree.color ?? 'default') === color.id;
          return <button
            key={color.id}
            className={`project-aow-worktree-color${color.id === 'default' ? ' default' : ''}${selected ? ' selected' : ''}`}
            type="button"
            role="menuitemradio"
            aria-checked={selected}
            disabled={savingAppearance}
            aria-label={color.label}
            title={color.label}
            style={color.value ? { '--worktree-color': color.value } as CSSProperties : undefined}
            onClick={() => { onColorChange(color.id); onClose(); }}
          ><span /></button>;
        })}
      </div>
    </div>
    <button className="danger" role="menuitem" disabled={state.worktree.is_main} title={state.worktree.is_main ? '主 Worktree 不能删除' : '删除 Worktree'} onClick={() => { onRemove(); onClose(); }}><Trash2 />{state.worktree.is_main ? '主 Worktree 不可删除' : '删除 Worktree'}</button>
  </div>;
}

function RemoveWorktreeDialog({ state, onClose, onSubmitted }: {
  state: RemoveWorktreeState;
  onClose: () => void;
  onSubmitted: (job: WorktreeRemovalJob) => void;
}) {
  const [preview, setPreview] = useState(state.preview);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const affectedTabs = preview.terminal_tabs + preview.agent_tabs;

  const remove = async () => {
    setBusy(true);
    setError('');
    try {
      const result = await aowApi.removeWorktree(state.project.id, state.worktree.path, preview.dirty);
      onSubmitted(result);
    } catch (reason) {
      setError(message(reason));
      try {
        setPreview(await aowApi.inspectWorktreeRemoval(state.project.id, state.worktree.path));
      } catch {
        // Keep the original preview when a refresh is no longer possible.
      }
      setBusy(false);
    }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={() => { if (!busy) onClose(); }}>
    <section className="project-aow-modal project-aow-dialog project-aow-remove-worktree" role="dialog" aria-modal="true" aria-labelledby="remove-worktree-title" onPointerDown={(event) => event.stopPropagation()}>
      <header><div><Trash2 /><strong id="remove-worktree-title">{preview.dirty ? '强制删除 Worktree' : '删除 Worktree'}</strong><span title={state.worktree.branch || basename(state.worktree.path)}>{state.worktree.branch || basename(state.worktree.path)}</span></div><button type="button" title="关闭" aria-label="关闭" disabled={busy} onClick={onClose}><X /></button></header>
      <div className="project-aow-dialog-body project-aow-remove-body">
        <p className="project-aow-form-intro">将删除 linked worktree 目录及其 Git worktree 元数据，不会删除对应 branch。</p>
        <code className="project-aow-remove-path" title={state.worktree.path}>{state.worktree.path}</code>
        {affectedTabs ? <p>同时关闭并清理该 Worktree 的 {preview.terminal_tabs} 个 Terminal、{preview.agent_tabs} 个 Agent 及其持久化会话元数据。</p> : <p>该 Worktree 当前没有需要关闭的 Terminal 或 Agent。</p>}
        {preview.dirty ? <div className="project-aow-dirty-warning">
          <strong>检测到 {preview.change_count} 项未提交内容，强制删除后无法从 AOW 恢复：</strong>
          <pre>{preview.changes.join('\n')}{preview.truncated ? '\n…更多变更未显示' : ''}</pre>
        </div> : <div className="project-aow-clean-note">Git 工作区当前没有未提交内容。</div>}
        {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
      </div>
      <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={busy} onClick={onClose}>取消</button><button type="button" className={`project-aow-dialog-button ${preview.dirty ? 'danger' : 'primary'}`} disabled={busy} onClick={() => void remove()}>{busy ? '清理中…' : preview.dirty ? '强制删除' : '删除 Worktree'}</button></footer>
    </section>
  </div>;
}

function ProjectMenu({ state, onClose, onCreateWorktree, onCleanup, onBindNotes, onRemove }: {
  state: ProjectMenuState;
  onClose: () => void;
  onCreateWorktree: () => void;
  onCleanup: () => void;
  onBindNotes: () => void;
  onRemove: () => void;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x: state.x, y: state.y });

  useLayoutEffect(() => {
    const bounds = menu.current?.getBoundingClientRect();
    if (!bounds) return;
    setPosition({
      x: Math.max(4, Math.min(state.x, window.innerWidth - bounds.width - 4)),
      y: Math.max(4, Math.min(state.y, window.innerHeight - bounds.height - 4)),
    });
  }, [state.x, state.y]);

  useEffect(() => {
    const close = () => onClose();
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') onClose(); };
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
  }, [onClose]);

  const action = (callback: () => void) => () => { onClose(); callback(); };

  return <div ref={menu} className="project-aow-context-menu project-aow-project-menu" style={{ left: position.x, top: position.y }} role="menu" aria-label={`${state.project.name} 操作`} onPointerDown={(event) => event.stopPropagation()}>
    <div className="project-aow-context-path" title={state.project.registered_path}>{state.project.name}</div>
    <button role="menuitem" onClick={action(onCreateWorktree)}><GitBranchPlus />创建 Worktree</button>
    <button role="menuitem" onClick={action(onCleanup)}><Trash2 />批量清理 Worktree</button>
    <button role="menuitem" onClick={action(onBindNotes)}><NotebookPen />绑定 Notes 目录</button>
    <button className="danger" role="menuitem" onClick={action(onRemove)}><Trash2 />移除 Project</button>
  </div>;
}

function BindNotesDialog({ project, onClose, onBound }: {
  project: AowProject;
  onClose: () => void;
  onBound: (project: AowProject) => void;
}) {
  const [path, setPath] = useState(project.notes_path);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const bind = async () => {
    const target = path.trim();
    if (!target.startsWith('/')) {
      setError('Notes 目录必须是服务端上的绝对路径。');
      return;
    }
    setBusy(true);
    setError('');
    try {
      onBound(await aowApi.bindNotes(project.id, target));
    } catch (reason) {
      setError(message(reason));
      setBusy(false);
    }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={() => { if (!busy) onClose(); }}>
    <section className="project-aow-modal project-aow-dialog project-aow-bind-notes" role="dialog" aria-modal="true" aria-labelledby="bind-notes-title" onPointerDown={(event) => event.stopPropagation()}>
      <header><div><NotebookPen /><strong id="bind-notes-title">绑定 Notes 目录</strong><span title={project.name}>{project.name}</span></div><button type="button" title="关闭" aria-label="关闭" disabled={busy} onClick={onClose}><X /></button></header>
      <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void bind(); }}>
        <div className="project-aow-dialog-body">
          <p className="project-aow-form-intro">绑定已有目录或输入新目录。目录不存在时会自动创建。</p>
          <label className="project-aow-dialog-field"><span>Notes 路径</span><input className="project-aow-dialog-monospace" aria-describedby="bind-notes-current" autoFocus autoComplete="off" spellCheck={false} value={path} disabled={busy} onChange={(event) => { setPath(event.target.value); setError(''); }} placeholder="/absolute/path/to/project-notes" required /></label>
          <small id="bind-notes-current" className="project-aow-dialog-path-hint">当前目录：<code title={project.notes_path}>{project.notes_path}</code></small>
          {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
        </div>
        <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={busy} onClick={onClose}>取消</button><button type="submit" className="project-aow-dialog-button primary" disabled={busy || !path.trim()}>{busy ? '绑定中…' : '绑定目录'}</button></footer>
      </form>
    </section>
  </div>;
}

function OpenFileDialog({ worktreePath, onClose, onOpen }: {
  worktreePath: string;
  onClose: () => void;
  onOpen: (path: string) => Promise<void>;
}) {
  const [path, setPath] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  const open = async () => {
    let target = path.trim();
    if (!target) return;
    setBusy(true);
    setError('');
    try {
      if (target.startsWith('~/')) {
        const home = await filesApi.listHomeDirectory();
        target = joinPath(home.path.replace(/\/+$/, '') || '/', target.slice(2));
      } else if (!target.startsWith('/')) {
        target = joinPath(worktreePath.replace(/\/+$/, '') || '/', target);
      }
      await onOpen(target);
      onClose();
    } catch (reason) {
      setError(message(reason));
      setBusy(false);
    }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={() => { if (!busy) onClose(); }}>
    <section className="project-aow-modal project-aow-open-file" role="dialog" aria-modal="true" aria-labelledby="open-file-title" onPointerDown={(event) => event.stopPropagation()}>
      <header><div><FileText /><strong id="open-file-title">打开文件</strong><span>输入绝对或相对路径</span></div><button title="关闭" disabled={busy} onClick={onClose}><X /></button></header>
      <form onSubmit={(event) => { event.preventDefault(); void open(); }}>
        <p className="project-aow-form-intro">相对路径基于当前 Worktree，~/ 指向用户主目录。可写文本支持编辑与自动保存。</p>
        <label><span>文件路径</span><input autoFocus spellCheck={false} value={path} disabled={busy} onChange={(event) => { setPath(event.target.value); setError(''); }} placeholder="src/main.ts、/absolute/path/to/file 或 ~/file" required /></label>
        {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
        <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button className="primary" disabled={busy || !path.trim()}>{busy ? '打开中…' : '打开文件'}</button></footer>
      </form>
    </section>
  </div>;
}

function RegisterProjectDialog({ onClose, onRegistered }: {
  onClose: () => void;
  onRegistered: (project: AowProject) => void | Promise<void>;
}) {
  const [step, setStep] = useState<'directory' | 'details'>('directory');
  const [path, setPath] = useState('');
  const [name, setName] = useState('');
  const [notesPath, setNotesPath] = useState('');
  const [notesBase, setNotesBase] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [browserPath, setBrowserPath] = useState('');
  const [browserPathInput, setBrowserPathInput] = useState('');
  const [browserDirectories, setBrowserDirectories] = useState<FileEntry[]>([]);
  const [browserLoading, setBrowserLoading] = useState(true);
  const [browserError, setBrowserError] = useState('');
  const browserRequest = useRef(0);
  const normalizedPath = path.trim();
  const previewName = name.trim() || basename(normalizedPath);

  useEffect(() => {
    let active = true;
    void aowApi.settings()
      .then((settings) => { if (active) setNotesBase(settings.notes_base); })
      .catch(() => undefined);
    return () => { active = false; };
  }, []);

  useEffect(() => {
    const close = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || busy) return;
      onClose();
    };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, [busy, onClose]);

  const loadBrowserDirectory = useCallback(async (directory?: string) => {
    const target = directory?.trim();
    const request = ++browserRequest.current;
    if (target && !target.startsWith('/')) {
      setBrowserError('请输入服务端上的绝对目录路径。');
      setBrowserLoading(false);
      return;
    }
    setBrowserLoading(true);
    setBrowserError('');
    try {
      const listing = target ? await filesApi.listDirectory(target) : await filesApi.listHomeDirectory();
      if (browserRequest.current !== request) return;
      setBrowserPath(listing.path);
      setBrowserPathInput(listing.path);
      setBrowserDirectories(listing.entries.filter((entry) => entry.kind === 'directory'));
      return listing.path;
    } catch (reason) {
      if (browserRequest.current === request) setBrowserError(message(reason));
    } finally {
      if (browserRequest.current === request) setBrowserLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadBrowserDirectory();
    return () => { browserRequest.current += 1; };
  }, [loadBrowserDirectory]);

  const chooseBrowserDirectory = async () => {
    const target = browserPathInput.trim();
    if (!target || browserLoading || browserError) return;
    // Validate a pasted address before advancing so the previous directory is
    // never silently used when the user skips the address bar's Go action.
    const selectedPath = target === browserPath ? browserPath : await loadBrowserDirectory(target);
    if (!selectedPath) return;
    setPath(selectedPath);
    setError('');
    setStep('details');
  };

  const register = async () => {
    if (step !== 'details' || busy || !normalizedPath) return;
    setBusy(true);
    setError('');
    try {
      const project = await aowApi.registerProject(normalizedPath, name, notesPath);
      await onRegistered(project);
    } catch (reason) {
      setError(message(reason));
      setBusy(false);
    }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={() => { if (!busy) onClose(); }}>
    <section className="project-aow-modal project-aow-dialog project-aow-register-project" role="dialog" aria-modal="true" aria-labelledby="register-project-title" onPointerDown={(event) => event.stopPropagation()}>
      <header><div><FolderGit2 /><strong id="register-project-title">注册项目</strong></div><button type="button" title="关闭" aria-label="关闭" disabled={busy} onClick={onClose}><X /></button></header>
      <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); if (step === 'directory') void chooseBrowserDirectory(); else void register(); }}>
        <div className="project-aow-dialog-body">
          <ol className="project-aow-register-steps" aria-label="注册步骤">
            <li aria-current={step === 'directory' ? 'step' : undefined}><span>1</span>选择仓库目录</li>
            <li aria-current={step === 'details' ? 'step' : undefined}><span>2</span>填写项目信息</li>
          </ol>
          {step === 'directory' ? <>
            <p className="project-aow-form-intro" id="register-project-directory-hint">浏览或粘贴主仓库 / Worktree 的绝对路径，点击「下一步」确认目录。</p>
            <section className="project-aow-directory-picker" aria-label="选择仓库目录">
              <div className="project-aow-directory-toolbar">
                <button type="button" title="服务端根目录" aria-label="服务端根目录" disabled={browserLoading || browserPath === '/'} onClick={() => void loadBrowserDirectory('/')}><span>/</span></button>
                <button type="button" title="上级目录" aria-label="上级目录" disabled={browserLoading || browserPath === '/'} onClick={() => void loadBrowserDirectory(dirname(browserPath || '/'))}><ArrowUp /></button>
                <div className="project-aow-directory-address"><input aria-label="目录路径" aria-describedby="register-project-directory-hint" autoFocus autoComplete="off" spellCheck={false} placeholder="粘贴目录的绝对路径" readOnly={browserLoading} value={browserPathInput} onChange={(event) => { setBrowserPathInput(event.target.value); setBrowserError(''); }} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); event.stopPropagation(); if (!browserLoading && browserPathInput.trim()) void loadBrowserDirectory(browserPathInput); } }} /><button type="button" disabled={browserLoading || !browserPathInput.trim()} onClick={() => void loadBrowserDirectory(browserPathInput)}>转到</button></div>
                <button type="button" title="刷新当前目录" aria-label="刷新当前目录" disabled={browserLoading || !browserPath} onClick={() => void loadBrowserDirectory(browserPath)}><RefreshCw className={browserLoading ? 'spinning' : ''} /></button>
              </div>
              <div className="project-aow-directory-list" aria-busy={browserLoading}>
                {browserLoading ? <div className="project-aow-directory-state"><LoaderCircle className="spinning" />正在读取目录…</div> : null}
                {!browserLoading && browserError ? <div className="project-aow-directory-state error" role="alert">{browserError}</div> : null}
                {!browserLoading && !browserError ? browserDirectories.map((directory) => <button type="button" key={directory.path} title={directory.path} onClick={() => void loadBrowserDirectory(directory.path)}><DirectoryTypeIcon /><span>{directory.name}</span><ChevronRight /></button>) : null}
                {!browserLoading && !browserError && !browserDirectories.length ? <div className="project-aow-directory-state">此目录下没有子目录</div> : null}
              </div>
              <div className="project-aow-directory-footer"><code title={browserPath}>{browserPath || (browserLoading ? '正在定位目录…' : '尚未选择目录')}</code></div>
            </section>
          </> : <>
            <p className="project-aow-form-intro">确认项目信息后注册，自动发现并列出关联的 Worktrees。</p>
            <div className="project-aow-register-preview" aria-label="项目注册预览">
              <FolderGit2 />
              <div><strong title={previewName}>{previewName}</strong><code title={normalizedPath}>{normalizedPath}</code></div>
            </div>
            <div className="project-aow-dialog-field">
              <label htmlFor="register-project-name">显示名称 <em>可选</em></label>
              <input id="register-project-name" autoFocus autoComplete="off" value={name} disabled={busy} onChange={(event) => { setName(event.target.value); if (error) setError(''); }} placeholder="默认使用仓库目录名" maxLength={120} />
            </div>
            <div className="project-aow-dialog-field">
              <label htmlFor="register-project-notes">Notes 路径 <em>可选</em></label>
              <input id="register-project-notes" className="project-aow-dialog-monospace" aria-describedby="register-project-notes-hint" autoComplete="off" spellCheck={false} value={notesPath} disabled={busy} onChange={(event) => { setNotesPath(event.target.value); if (error) setError(''); }} placeholder="留空时自动映射" />
              <small id="register-project-notes-hint">可绑定已有目录；留空时按仓库地址映射，本地仓库映射到 localhost。{notesBase ? <span className="project-aow-dialog-path-hint">根目录：<code title={notesBase}>{notesBase}</code></span> : null}</small>
            </div>
            {error ? <div className="project-aow-error project-aow-register-error" role="alert">{error}</div> : null}
          </>}
        </div>
        <footer className="project-aow-dialog-footer">
          <button type="button" className="project-aow-dialog-button" disabled={busy} onClick={onClose}>取消</button>
          {step === 'directory' ? <button key="next" type="submit" className="project-aow-dialog-button primary" disabled={browserLoading || !!browserError || !browserPathInput.trim()}>下一步<ChevronRight /></button> : <>
            <button type="button" className="project-aow-dialog-button" disabled={busy} onClick={() => { setStep('directory'); setError(''); }}>上一步</button>
            <button key="register" type="submit" className="project-aow-dialog-button primary" disabled={busy || !normalizedPath}>{busy ? <><LoaderCircle className="spinning" />正在注册…</> : <><Plus />注册项目</>}</button>
          </>}
        </footer>
      </form>
    </section>
  </div>;
}

function CreateWorktreeDialog({ project, onClose, onCreated }: {
  project: AowProject;
  onClose: () => void;
  onCreated: (project: AowProject, worktree: AowWorktree) => void;
}) {
  const defaultBase = project.worktrees.find((worktree) => worktree.is_main)?.branch
    || project.worktrees.find((worktree) => !worktree.detached)?.branch
    || 'HEAD';
  const [branch, setBranch] = useState('');
  const [baseRef, setBaseRef] = useState(defaultBase);
  const [pullFirst, setPullFirst] = useState(true);
  const [path, setPath] = useState(() => suggestedWorktreePath(project, ''));
  const [pathEdited, setPathEdited] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const mainPath = project.worktrees.find((worktree) => worktree.is_main)?.path ?? project.registered_path;
  const command = `${pullFirst ? `git -C ${shellQuote(mainPath)} pull && ` : ''}git -C ${shellQuote(project.registered_path)} worktree add -b ${shellQuote(branch || '<new-branch>')} -- ${shellQuote(path || '<absolute-path>')} ${shellQuote(baseRef || 'HEAD')}`;

  const changeBranch = (value: string) => {
    setBranch(value);
    if (!pathEdited) setPath(suggestedWorktreePath(project, value));
  };

  const create = async () => {
    setBusy(true);
    setError('');
    try {
      const result = await aowApi.createWorktree(project.id, { branch, baseRef, path, pullFirst });
      onCreated(result.project, result.worktree);
    } catch (reason) {
      setError(`Worktree 创建失败：${message(reason)}`);
      setBusy(false);
    }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={() => { if (!busy) onClose(); }}>
    <section className="project-aow-modal project-aow-dialog project-aow-create-worktree" role="dialog" aria-modal="true" aria-labelledby="create-worktree-title" onPointerDown={(event) => event.stopPropagation()}>
      <header><div><GitBranchPlus /><strong id="create-worktree-title">创建 Worktree</strong><span title={project.name}>{project.name}</span></div><button type="button" title="关闭" aria-label="关闭" disabled={busy} onClick={onClose}><X /></button></header>
      <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void create(); }}>
        <div className="project-aow-dialog-body">
          <p className="project-aow-form-intro">从指定分支或提交创建新的 Worktree。</p>
          <label className="project-aow-dialog-field"><span>新分支</span><input className="project-aow-dialog-monospace" autoFocus autoComplete="off" spellCheck={false} value={branch} onChange={(event) => changeBranch(event.target.value)} placeholder="feature/my-change" required disabled={busy} /></label>
          <label className="project-aow-dialog-field"><span>起始分支或提交</span><input className="project-aow-dialog-monospace" autoComplete="off" spellCheck={false} value={baseRef} onChange={(event) => setBaseRef(event.target.value)} placeholder="main、origin/main 或 commit SHA" required disabled={busy} /></label>
          <label className="project-aow-dialog-checkbox" title="创建前先对主仓库执行 git pull，失败则取消创建。"><input type="checkbox" checked={pullFirst} onChange={(event) => setPullFirst(event.target.checked)} disabled={busy} /><span>创建前更新主仓库（git pull）</span></label>
          <div className="project-aow-dialog-field">
            <label htmlFor="create-worktree-path">Worktree 路径</label>
            <input id="create-worktree-path" className="project-aow-dialog-monospace" aria-describedby="create-worktree-path-hint" autoComplete="off" spellCheck={false} value={path} onChange={(event) => { setPathEdited(true); setPath(event.target.value); }} placeholder="/absolute/path/to/worktree" required disabled={busy} />
            <small id="create-worktree-path-hint">目标路径必须是尚不存在的绝对路径。</small>
          </div>
          <details className="project-aow-command-preview"><summary>查看 Git 命令</summary><code>{command}</code></details>
          {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
        </div>
        <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={busy} onClick={onClose}>取消</button><button type="submit" className="project-aow-dialog-button primary" disabled={busy || !branch.trim() || !baseRef.trim() || !path.trim()}>{busy ? '创建中…' : '创建 Worktree'}</button></footer>
      </form>
    </section>
  </div>;
}

function SettingsDialog({ agents, onClose: closeDialog, onReload, onNodesChange }: { agents: AowAgent[]; onClose: () => void; onReload: (notesMoved?: boolean) => Promise<void>; onNodesChange: (addresses: string[]) => void }) {
  const [section, setSection] = useState<'notes' | 'editor' | 'environment' | 'agents' | 'im' | 'notifications' | 'nodes' | 'review' | 'configuration'>('notes');
  const [reviewDirty, setReviewDirty] = useState(false);
  const [configurationDirty, setConfigurationDirty] = useState(false);
  const onClose = () => { if (!(reviewDirty || configurationDirty) || window.confirm(configurationDirty ? '设置有未保存的修改，是否放弃并关闭？' : 'Provider 有未保存的修改，是否放弃并关闭？')) closeDialog(); };
  const { updateEditorSettings } = useEditorSettings();
  const [editorWordWrap, setEditorWordWrap] = useState(false);
  const [editorSaved, setEditorSaved] = useState(false);
  const [settings, setSettings] = useState<AowSettings>();
  const [nodeAddresses, setNodeAddresses] = useState('');
  const [nodesSaved, setNodesSaved] = useState(false);
  const [notesBase, setNotesBase] = useState('');
  const [executionPath, setExecutionPath] = useState('');
  const [environmentSaved, setEnvironmentSaved] = useState(false);
  const [settingsLoading, setSettingsLoading] = useState(true);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [editingAgentId, setEditingAgentId] = useState<string>();
  const [agentType, setAgentType] = useState<AowAgent['agent_type'] | ''>('');
  const [displayName, setDisplayName] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState('[]');
  const [env, setEnv] = useState('{}');
  const [agentSaved, setAgentSaved] = useState('');
  const agentForm = useRef<HTMLFormElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    void aowApi.settings()
      .then((next) => {
        if (!active) return;
        setSettings(next);
        setNotesBase(next.notes_base);
        setNodeAddresses((next.node_addresses ?? []).join('\n'));
        onNodesChange(next.node_addresses ?? []);
        setExecutionPath((next.execution_path ?? []).join('\n'));
        setEditorWordWrap(next.editor?.word_wrap ?? false);
        updateEditorSettings(next.editor ?? defaultEditorSettings);
      })
      .catch((reason) => { if (active) setError(message(reason)); })
      .finally(() => { if (active) setSettingsLoading(false); });
    return () => { active = false; };
  }, [updateEditorSettings, onNodesChange]);

  const saveNodes = async () => {
    setError('');
    setNodesSaved(false);
    try {
      const addresses = parseNodeAddresses(nodeAddresses);
      setSettingsBusy(true);
      const next = await aowApi.updateSettings({ nodeAddresses: addresses });
      setSettings(next);
      setNodeAddresses(next.node_addresses.join('\n'));
      onNodesChange(next.node_addresses);
      setNodesSaved(true);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const saveEditor = async () => {
    setSettingsBusy(true);
    setError('');
    setEditorSaved(false);
    try {
      const next = await aowApi.updateSettings({ editor: { word_wrap: editorWordWrap } });
      setSettings(next);
      setEditorWordWrap(next.editor.word_wrap);
      updateEditorSettings(next.editor);
      setEditorSaved(true);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const saveNotesRoot = async () => {
    const target = notesBase.trim();
    if (!target.startsWith('/')) {
      setError('Notes 根目录必须是服务端上的绝对路径。');
      return;
    }
    setSettingsBusy(true);
    setError('');
    try {
      const next = await aowApi.updateSettings({ notesBase: target });
      setSettings(next);
      setNotesBase(next.notes_base);
      await onReload(true);
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const readEnvironment = async () => {
    setSettingsBusy(true);
    setError('');
    setEnvironmentSaved(false);
    try {
      setExecutionPath((await aowApi.discoveredPath()).join('\n'));
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const saveEnvironment = async () => {
    const paths = executionPath.split('\n').map((path) => path.trim()).filter(Boolean);
    if (!paths.length || paths.some((path) => !path.startsWith('/') || path.includes(':'))) {
      setError('PATH 每行填写一个服务端绝对目录路径，不使用冒号分隔。');
      return;
    }
    setSettingsBusy(true);
    setError('');
    setEnvironmentSaved(false);
    try {
      const next = await aowApi.updateSettings({ executionPath: paths });
      setSettings(next);
      setExecutionPath(next.execution_path.join('\n'));
      setEnvironmentSaved(true);
      await onReload();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setSettingsBusy(false);
    }
  };

  const resetAgentForm = () => {
    setEditingAgentId(undefined);
    setAgentType('');
    setDisplayName('');
    setCommand('');
    setArgs('[]');
    setEnv('{}');
    setAgentSaved('');
    setError('');
  };

  const editAgent = (agent: AowAgent) => {
    setEditingAgentId(agent.id);
    setAgentType(aowAgentType(agent) ?? '');
    setDisplayName(agent.display_name);
    setCommand(agent.command ?? agent.executable ?? '');
    setArgs(JSON.stringify(agent.args));
    setEnv(JSON.stringify(agent.env ?? {}, null, 2));
    setAgentSaved('');
    setError('');
    agentForm.current?.querySelector('select')?.scrollIntoView({ block: 'nearest' });
  };

  const removeAgent = async (id: string) => {
    setBusy(true);
    setError('');
    setAgentSaved('');
    try {
      await agentsApi.removeAgent(id);
      if (editingAgentId === id) resetAgentForm();
      await onReload();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    if (!agentType) {
      setError('请选择 Agent 类型。');
      return;
    }
    setBusy(true);
    setError('');
    setAgentSaved('');
    try {
      const parsed = JSON.parse(args) as unknown;
      if (!Array.isArray(parsed) || parsed.some((value) => typeof value !== 'string')) {
        throw new Error('Arguments 必须是字符串 JSON 数组');
      }
      const environment = JSON.parse(env.trim() || '{}') as unknown;
      if (!environment || typeof environment !== 'object' || Array.isArray(environment)
        || Object.entries(environment).some(([key, value]) => !/^[A-Za-z0-9_]+$/.test(key) || typeof value !== 'string' || value.includes('\0'))) {
        throw new Error('Environment variables 必须是变量名到字符串值的 JSON 对象');
      }
      await agentsApi.registerAgent({
        id: editingAgentId,
        agentType,
        displayName, command, args: parsed,
        env: environment as Record<string, string>,
      });
      resetAgentForm();
      setAgentSaved(`${displayName} 配置已保存。`);
      await onReload();
    } catch (reason) {
      setError(message(reason));
    } finally {
      setBusy(false);
    }
  };

  return <div className="project-aow-modal-backdrop" onPointerDown={() => { if (!busy && !settingsBusy) onClose(); }}>
    <section className="project-aow-modal project-aow-dialog project-aow-settings" role="dialog" aria-modal="true" aria-labelledby="aow-settings-title" onPointerDown={(event) => event.stopPropagation()}>
      <header><div><Settings /><strong id="aow-settings-title">设置</strong></div><button type="button" title="关闭" aria-label="关闭" disabled={busy || settingsBusy} onClick={onClose}><X /></button></header>
      <div className="project-aow-settings-body">
        <nav className="project-aow-settings-nav" aria-label="设置分类">
          <button disabled={busy || settingsBusy} aria-current={section === 'configuration' ? 'page' : undefined} className={section === 'configuration' ? 'active' : ''} onClick={() => { setSection('configuration'); setError(''); }}><Settings /><span><strong>Configuration</strong><small>选择配置仓库和版本</small></span></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'nodes' ? 'page' : undefined} className={section === 'nodes' ? 'active' : ''} onClick={() => { setSection('nodes'); setError(''); }}><Network /><span><strong>Nodes</strong><small>配置其他 AOW 节点</small></span></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'editor' ? 'page' : undefined} className={section === 'editor' ? 'active' : ''} onClick={() => { setSection('editor'); setError(''); }}><FileText /><span><strong>Editor</strong><small>配置文件编辑器</small></span></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'notes' ? 'page' : undefined} className={section === 'notes' ? 'active' : ''} onClick={() => setSection('notes')}><NotebookPen /><span><strong>Notes</strong><small>设置默认 Notes 根目录</small></span></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'environment' ? 'page' : undefined} className={section === 'environment' ? 'active' : ''} onClick={() => { setSection('environment'); setError(''); }}><SquareTerminal /><span><strong>Environment</strong><small>配置全局执行 PATH</small></span></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'agents' ? 'page' : undefined} className={section === 'agents' ? 'active' : ''} onClick={() => setSection('agents')}><Bot /><span><strong>Agents</strong><small>配置 3 种支持的 Agent</small></span><i>{agents.filter((agent) => agent.available).length}</i></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'im' ? 'page' : undefined} className={section === 'im' ? 'active' : ''} onClick={() => setSection('im')}><MessageSquare /><span><strong>IM</strong><small>配置消息机器人</small></span></button>
          <button disabled={busy || settingsBusy} aria-current={section === 'notifications' ? 'page' : undefined} className={section === 'notifications' ? 'active' : ''} onClick={() => setSection('notifications')}><Bell /><span><strong>通知</strong><small>选择任务完成通知方式</small></span></button>
          <button type="button" className={section === 'review' ? 'active' : ''} disabled={busy || settingsBusy} onClick={() => setSection('review')}><GitPullRequest /><span><strong>Pull Requests</strong><small>Provider、CLI 和脚本</small></span></button>
        </nav>
        <div className="project-aow-settings-content">
          <div className="review-provider-settings-host" hidden={section !== 'review'}><ReviewProviderSettings active={section === 'review'} onBusyChange={setSettingsBusy} onDirtyChange={setReviewDirty} /></div>
          <div className="configuration-settings-host" hidden={section !== 'configuration'}><ConfigurationSettings active={section === 'configuration'} onBusyChange={setSettingsBusy} onDirtyChange={setConfigurationDirty} /></div>
          {section === 'review' || section === 'configuration' ? null : section === 'nodes' ? (
            <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void saveNodes(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Nodes</h2><p>配置其他机器上部署的 AOW，点击左上角 Logo 或 AOW 文字即可切换。</p></div></div>
                <label className="project-aow-dialog-field"><span>节点地址（每行一个）</span><textarea aria-label="节点地址" spellCheck={false} rows={10} value={nodeAddresses} disabled={settingsLoading || settingsBusy || !settings} onChange={event => { setNodeAddresses(event.target.value); setNodesSaved(false); setError(''); }} placeholder={'https://aow-a.example.com\nhttp://192.168.1.20:8080'} /></label>
                <p className="project-aow-form-intro">填写完整的 http:// 或 https:// 地址，可以包含当前节点。同一份列表可复制到所有节点；下拉菜单会按协议、域名/IP 和端口自动过滤当前节点。留空并保存可清空列表。</p>
                {nodesSaved ? <p role="status">节点地址已保存。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !settings}>{settingsBusy ? '保存中…' : '保存'}</button></footer>
            </form>
          ) : section === 'im' || section === 'notifications' ? <NotificationSettingsPanel section={section} onBusyChange={setSettingsBusy} /> : section === 'editor' ? (
            <form className="project-aow-dialog-form" onSubmit={event => { event.preventDefault(); void saveEditor(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Editor</h2><p>设置文件编辑器的全局默认行为。</p></div></div>
                <label className="project-aow-dialog-checkbox"><input type="checkbox" checked={editorWordWrap} disabled={settingsLoading || settingsBusy || !settings} onChange={event => { setEditorWordWrap(event.target.checked); setEditorSaved(false); setError(''); }} /><span>Word Wrap（自动换行）</span></label>
                <p className="project-aow-form-intro">开启后，长行会根据编辑区宽度自动折行。每个文件 Tab 可通过右上角开关临时切换，刷新或重新打开后恢复使用全局配置。</p>
                {editorSaved ? <p role="status">已保存 Editor 配置。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !settings}>{settingsBusy ? '保存中…' : '保存'}</button></footer>
            </form>
          ) : section === 'notes' ? <>
            <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void saveNotesRoot(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Notes</h2><p>新注册项目在未指定 Notes path 时，会以仓库地址映射到这个根目录下。</p></div></div>
                <label className="project-aow-dialog-field"><span>Notes root</span><input className="project-aow-dialog-monospace" autoFocus spellCheck={false} value={notesBase} disabled={settingsLoading || settingsBusy} onChange={(event) => { setNotesBase(event.target.value); setError(''); }} placeholder="/absolute/path/to/aow" required /></label>
                <small className="project-aow-dialog-path-hint">当前根目录：<code title={settings?.notes_base}>{settings?.notes_base ?? '加载中…'}</code></small>
                <p className="project-aow-form-intro">保存后，使用默认映射的项目会迁移已有 Notes；显式绑定的自定义目录保持原位置。目标目录冲突时会保留原绑定并提示。</p>
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !notesBase.trim()}>{settingsBusy ? '保存中…' : '保存'}</button></footer>
            </form>
          </> : section === 'environment' ? <>
            <form className="project-aow-dialog-form" onSubmit={(event) => { event.preventDefault(); void saveEnvironment(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Environment</h2><p>统一配置新启动的 Agent 和自动化任务使用的命令搜索路径。</p></div></div>
                <label className="project-aow-dialog-field"><span>PATH 目录（从上到下优先）</span><textarea aria-label="PATH 目录" spellCheck={false} rows={10} value={executionPath} disabled={settingsLoading || settingsBusy} onChange={(event) => { setExecutionPath(event.target.value); setEnvironmentSaved(false); setError(''); }} placeholder={'/opt/python/3.11/bin\n/usr/local/bin\n/usr/bin\n/bin'} required /></label>
                <p className="project-aow-form-intro">每行一个服务器上的绝对目录。要优先使用某个 Python，请把包含 python3 的目录放在前面；这里不填写可执行文件，也不展开 ~、$HOME 或 $PATH。</p>
                <p className="project-aow-form-intro">保存后对后续执行生效，已有任务无需重新保存。正在运行的任务保持原环境。</p>
                {environmentSaved ? <p role="status">执行环境已保存。</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer"><button type="button" className="project-aow-dialog-button" disabled={settingsLoading || settingsBusy} onClick={() => void readEnvironment()}><RefreshCw size={14} />从本机环境读取</button><button type="submit" className="project-aow-dialog-button primary" disabled={settingsLoading || settingsBusy || !executionPath.trim()}>{settingsBusy ? '处理中…' : '保存'}</button></footer>
            </form>
          </> : <>
            <form className="project-aow-dialog-form" ref={agentForm} onSubmit={(event) => { event.preventDefault(); void save(); }}>
              <div className="project-aow-dialog-body">
                <div className="project-aow-settings-heading"><div><h2>Agents</h2><p>目前支持 Claude Code、Codex 和 TraeCode CLI。每种类型可注册多个配置。</p></div><button type="button" className="project-aow-dialog-button" title="重新探测本地 Agent" disabled={busy} onClick={() => void onReload().catch((reason) => setError(message(reason)))}><RefreshCw />刷新</button></div>
                <div className="project-aow-agent-list">
                  {agents.map((agent) => <div className="project-aow-agent-row" key={agent.id}>
                    <span className={`project-aow-agent-dot ${agent.available ? 'available' : ''}`} />
                    <AgentIcon agentId={aowAgentType(agent)} />
                    <div><strong>{agent.display_name}</strong><small>{agentTypes.find(type => type.id === aowAgentType(agent))?.label ?? '未设置类型，请编辑补选'}</small><code>{agent.executable ?? '未找到 executable'}</code></div>
                    <small>{agent.source === 'detected' ? 'Auto detected' : 'Configured'}</small>
                    <button type="button" className="project-aow-agent-edit" title={`编辑 ${agent.display_name}`} aria-pressed={editingAgentId === agent.id} disabled={busy} onClick={() => editAgent(agent)}><Pencil /></button>
                    {agent.source === 'configured' ? <button type="button" title="移除配置" disabled={busy} onClick={() => void removeAgent(agent.id)}><Trash2 /></button> : null}
                  </div>)}
                  {!agents.length ? <p className="project-aow-empty">尚未发现本地 Agent，可在下方注册。</p> : null}
                </div>
                <h3>{editingAgentId ? '编辑 Agent 配置' : '注册 Agent 配置'}</h3>
                <label className="project-aow-dialog-field"><span>Agent 类型 <em>必填</em></span><div className="project-aow-agent-type"><AgentIcon agentId={agentType} /><select aria-label="Agent 类型" value={agentType ?? ''} disabled={busy || !!builtinAgentType(editingAgentId)} onChange={(event) => { setAgentType(builtinAgentType(event.target.value) ?? ''); setError(''); }} required><option value="" disabled>请选择 Agent 类型</option>{agentTypes.map(type => <option key={type.id} value={type.id}>{type.label}</option>)}</select></div></label>
                <p className="project-aow-form-intro">选择类型后，可自由配置名称、启动命令、参数和环境变量。</p>
                <label className="project-aow-dialog-field"><span>Display name</span><input value={displayName} disabled={busy} onChange={(event) => setDisplayName(event.target.value)} placeholder="例如：工作用 Codex" required /></label>
                <label className="project-aow-dialog-field"><span>Executable</span><input className="project-aow-dialog-monospace" spellCheck={false} value={command} disabled={busy} onChange={(event) => setCommand(event.target.value)} placeholder="命令名或 /absolute/path" required /></label>
                <label className="project-aow-dialog-field"><span>Arguments</span><input className="project-aow-dialog-monospace" spellCheck={false} value={args} disabled={busy} onChange={(event) => setArgs(event.target.value)} placeholder='["--flag"]' /></label>
                <label className="project-aow-dialog-field"><span>Environment variables</span><textarea aria-label="Environment variables" spellCheck={false} rows={4} value={env} disabled={busy} onChange={(event) => setEnv(event.target.value)} placeholder={'{\n  "BASE_URL": "https://example.com"\n}'} /></label>
                <p className="project-aow-form-intro">自动继承启动环境。这里只填写需要新增或覆盖的变量（JSON 对象）；留空或填写 {'{}'} 即可保留继承的环境。</p>
                <p className="project-aow-form-intro">保存后用于新启动的终端 Agent。PATH 统一在 Environment 中配置。移除内置 Agent 的配置后会恢复自动探测。</p>
                {agentSaved ? <p role="status">{agentSaved}</p> : null}
                {error ? <div className="project-aow-error" role="alert">{error}</div> : null}
              </div>
              <footer className="project-aow-dialog-footer">{editingAgentId ? <button type="button" className="project-aow-dialog-button" disabled={busy} onClick={resetAgentForm}>取消编辑</button> : null}<button type="submit" className="project-aow-dialog-button primary" disabled={busy || !agentType}>{busy ? '保存中…' : editingAgentId ? '保存配置' : '注册'}</button></footer>
            </form>
          </>}
        </div>
      </div>
    </section>
  </div>;
}

function LeftSidebarToggle({ expanded = false, onClick }: { expanded?: boolean; onClick: () => void }) {
  const label = expanded ? '隐藏左侧栏' : '显示左侧栏';
  return <button type="button" className="project-aow-left-toggle" title={label} aria-label={label}
    aria-expanded={expanded} aria-controls="aow-project-sidebar" onClick={onClick}>
    {expanded ? <PanelLeftClose /> : <PanelLeftOpen />}
  </button>;
}

function WorktreeUnreadBadge({ count = 0 }: { count?: number }) {
  if (!count) return null;
  const label = `${count} 条未读通知`;
  return <span className="project-aow-worktree-unread" title={label} aria-label={label}>{count}</span>;
}

interface WorkspaceSurfaceProps {
  initialEntry?: ResolvedTab;
  locationSource?: 'center' | 'floating';
  worktree: AowWorktree;
  project: AowProject;
  active: boolean;
  agents: AowAgent[];
  onShowLeftSidebar?: () => void;
  rightSidebarVisible: boolean;
  onHideRightSidebar: () => void;
  onShowRightSidebar: () => void;
  onStartRightResize: (event: ReactPointerEvent<HTMLDivElement>) => void;
  notesRefresh: ExplorerRefresh;
  onNotesChanged: (projectId: string, directory: string) => void;
  onResourcesChanged?: (path: string, resources?: WorktreeResourceState) => void;
}

const WorkspaceSurface = memo(function WorkspaceSurface({
  worktree, project, active, agents, onShowLeftSidebar, rightSidebarVisible, onHideRightSidebar,
  onShowRightSidebar, onStartRightResize, notesRefresh, onNotesChanged: onProjectNotesChanged, onResourcesChanged, initialEntry, locationSource,
}: WorkspaceSurfaceProps) {
  const floating = useFloatingWorkspace();
  const [browserOpened, setBrowserOpened] = useState(() => !!project.builtin && floating.tabs.some(tab => tab.id === 'system-files'));
  const [browserTab, setBrowserTab] = useState(browserOpened);
  const restoring = useRef(true);
  const [restored, setRestored] = useState(false);
  const initialEntryApplied = useRef<ResolvedTab | undefined>(undefined);
  const [browserPath, setBrowserPath] = useState('');
  const [browserRequest, setBrowserRequest] = useState<{ path: string }>();
  const [automationLocations, setAutomationLocations] = useState<Record<string, AutomationLocation>>({});
  const syncTabLocation = useAowTabLocation();
  const reportTabLocationError = useAowTabLocationError();
  const [tabRevealRequest, setTabRevealRequest] = useState<TabRevealRequest>();
  const routeToFloating = useCallback((id: string, requested = openingInFloatingWorkspace()) => {
    if (restoring.current) return;
    if (requested || project.builtin || floating.contains(worktree.path, id)) floating.add(worktree.path, id);
    else setTabRevealRequest({ tabId: id });
  }, [floating.add, floating.contains, project.builtin, worktree.path]);
  const isFloating = (id: string) => floating.tabs.some(tab => tab.workspace === worktree.path && tab.id === id);
  const hostedLocation = (id: string) => floating.hostedTabs.find(tab => tab.workspace === worktree.path && tab.id === id);
  const hostedTabs = useMemo(() => floating.hostedTabs.filter(tab => tab.host === worktree.path), [floating.hostedTabs, worktree.path]);
  const hostedTab = (id: string) => hostedTabs.find(tab => tab.id === id);
  const floatingId = floating.active?.workspace === worktree.path ? floating.active.id : undefined;
  const tabVisible = (id: string) => {
    const hosted = hostedLocation(id);
    return hosted ? floating.hosts[hosted.host]?.activeId === id : isFloating(id) ? floatingId === id : activeCenterId === id;
  };
  const tabLive = (id: string) => {
    const hosted = hostedLocation(id);
    return tabVisible(id) && (hosted ? !!floating.hosts[hosted.host]?.visible : isFloating(id) ? floating.visible : active);
  };
  const projectContent = (id: string, content: React.ReactNode) => {
    // The owner keeps the terminal state and callbacks; only its render target moves.
    const hosted = hostedLocation(id);
    if (hosted) {
      const host = floating.hosts[hosted.host];
      return host?.portal ? createPortal(content, host.portal, worktree.path + id) : null;
    }
    return isFloating(id) && floating.portal
      ? createPortal(<div className="floating-workspace-host" hidden={floatingId !== id}>{content}</div>, floating.portal, worktree.path + id)
      : content;
  };
  const onNotesChanged = useCallback((directory: string) => onProjectNotesChanged(project.id, directory), [project.id, onProjectNotesChanged]);
  const terminals = useTerminals(worktree.path, true);
  const [terminalVisibility, setTerminalVisibility] = useState<Record<string, boolean>>({});
  const openedTerminals = useMemo(() => terminals.tabs.filter(tab => terminalVisibility[tab.id]
    ?? (floating.hostedTabs.some(item => item.workspace === worktree.path && item.targetId === tab.id)
      || floating.tabs.some(item => item.workspace === worktree.path && item.targetId === tab.id) || !isCliTerminal(tab))),
  [terminals.tabs, terminalVisibility, floating.hostedTabs, floating.tabs, worktree.path]);
  const [documents, setDocuments] = useWorkspaceDocuments(worktree.path);
  const { wordWrapFor, setWordWrap } = useWordWrapOverrides(documents);
  const [sessionPreviews, setSessionPreviews] = useState<SessionPreview[]>([]);
  const [openPullRequests, setOpenPullRequests] = useState<(PullRequestSummary & { repository?: string })[]>([]);
  const [automationTasks, setAutomationTasks] = useState<Record<string, AutomationTask>>({});
  const [openAutomationTaskIds, setOpenAutomationTaskIds] = useState<string[]>([]);
  const [automationRefreshKey, setAutomationRefreshKey] = useState(0);
  const [activeDocumentId, setActiveDocumentId] = useState<string>();
  const [activeCenterId, setActiveCenterId] = useState<string>();
  const [hostPortal, setHostPortal] = useState<HTMLDivElement | null>(null);
  useLayoutEffect(() => {
    floating.publishHost(worktree.path, { portal: hostPortal, activeId: activeCenterId, visible: active });
  }, [floating.publishHost, worktree.path, hostPortal, activeCenterId, active]);
  useEffect(() => () => floating.publishHost(worktree.path), [floating.publishHost, worktree.path]);
  const [rightView, setRightView] = useState<RightView>('terminals');
  const canShowAllTerminals = worktree.is_main && !project.builtin;
  const terminalScopeKey = `aow-terminal-scope:${worktree.path}`;
  const [terminalScopes, setTerminalScopes] = useState(() => {
    const stored = readStored<boolean | { user?: boolean; cli?: boolean }>(terminalScopeKey, false);
    return {
      user: typeof stored === 'boolean' ? stored : stored.user === true,
      cli: typeof stored === 'boolean' ? stored : stored.cli === true,
    };
  });
  const terminalSortKey = `aow-terminal-sort:${worktree.path}`;
  const [terminalSorts, setTerminalSorts] = useState<Record<'user' | 'cli', TerminalSort>>(() => {
    const stored = readStored<{ user?: TerminalSort; cli?: TerminalSort } | null>(terminalSortKey, null);
    return { user: stored?.user === 'createdAt' ? 'createdAt' : 'branch', cli: stored?.cli === 'createdAt' ? 'createdAt' : 'branch' };
  });
  const showAllTerminals = canShowAllTerminals && (terminalScopes.user || terminalScopes.cli);
  const projectTerminals = useProjectTerminals(project.worktrees, showAllTerminals && active && rightSidebarVisible && rightView === 'terminals');
  const panelTerminals = showAllTerminals
    ? [...terminals.tabs, ...projectTerminals.tabs.filter(tab => tab.workspace_root !== worktree.path
      && terminalScopes[isCliTerminal(tab) ? 'cli' : 'user'])]
    : terminals.tabs;
  const [filesExplorerCollapsed, setFilesExplorerCollapsed] = useState<boolean>();
  const [notesExplorerCollapsed, setNotesExplorerCollapsed] = useState<boolean>();
  const [showOpenFile, setShowOpenFile] = useState(false);
  const [openFileFloating, setOpenFileFloating] = useState(false);
  const [explorerRefresh, setExplorerRefresh] = useState({ generation: 0, directory: worktree.path });
  const [sourceControlRefresh, setSourceControlRefresh] = useState(0);
  const [operationError, setOperationError] = useState('');
  const documentsRef = useRef<OpenDocument[]>([]);
  const autoSaveTimers = useRef(new Map<string, number>());
  const autoSaveFailedIds = useRef(failedDocuments);
  const savingDocumentIds = useRef(savingDocuments);
  const autoSavePendingAfterSave = useRef(new Set<string>());

  const documentRefreshes = useRef(new Map<string, { instanceId?: number; controller: AbortController }>());
  const refreshedPreviewUrls = useRef(new Set<string>());
  const sessionSnapshotRequestIds = useRef(new Map<string, number>());

  const terminalTabs = useMemo(() => openedTerminals.map((tab) => terminalCenterTab(tab, terminals.detectedAgents, terminals.terminalTitles)), [openedTerminals, terminals.detectedAgents, terminals.terminalTitles]);
  const documentTabs = useMemo<CenterTab[]>(() => documents.map((document) => ({
    id: `document:${document.id}`,
    kind: document.id.startsWith('diff:') || document.id.startsWith('commit-diff:') ? 'diff' : 'file',
    label: document.name,
    targetId: document.id,
    renameable: document.explorerSource !== 'external',
    dirty: document.dirty,
  })), [documents]);
  const sessionTabs = useMemo<CenterTab[]>(() => sessionPreviews.map(({ session }) => ({
    id: `session:${session.id}`,
    kind: 'session',
    label: session.title,
    targetId: session.id,
  })), [sessionPreviews]);
  const automationTabs = useMemo<CenterTab[]>(() => openAutomationTaskIds.map((id) => ({
    id: `automation:${id}`, kind: 'automation', label: automationTasks[id]?.name ?? '自动化任务', targetId: id,
    automationKind: automationTasks[id]?.kind,
  })), [automationTasks, openAutomationTaskIds]);
  const pullRequestTabs = useMemo<CenterTab[]>(() => openPullRequests.map((pr) => ({
    id: prTabId(pr.repository ?? worktree.path, pr.number, pr), kind: 'pullRequest', label: `PR #${pr.number}`, targetId: prTabId(pr.repository ?? worktree.path, pr.number, pr),
  })), [openPullRequests]);
  const ownedCenterTabs = useMemo(() => groupWorkspaceTabs([
    ...(browserTab ? [{ id: 'system-files', kind: 'browser' as const, label: '系统文件浏览器', targetId: 'system-files' }] : []),
    ...terminalTabs, ...sessionTabs, ...automationTabs, ...pullRequestTabs, ...documentTabs,
  ]).flatMap(group => group.tabs), [automationTabs, documentTabs, pullRequestTabs, sessionTabs, terminalTabs, browserTab]);
  const centerTabs = useMemo(() => groupWorkspaceTabs([...ownedCenterTabs, ...hostedTabs])
    .flatMap(group => group.tabs), [ownedCenterTabs, hostedTabs]);
  const hasFrontendTabs = centerTabs.length > 0 ? true : restored ? false : undefined;
  const hasTerminalInstances = terminals.tabs.length > 0 ? true : terminals.loaded && !terminals.error ? false : undefined;
  useEffect(() => {
    onResourcesChanged?.(worktree.path, { hasFrontendTabs, hasTerminalInstances });
  }, [hasFrontendTabs, hasTerminalInstances, onResourcesChanged, worktree.path]);
  useEffect(() => () => onResourcesChanged?.(worktree.path), [onResourcesChanged, worktree.path]);
  const sidebarActiveId = project.builtin ? floatingId : activeCenterId;
  const activeTerminalId = sidebarActiveId?.startsWith('terminal:')
    && (project.builtin || (!isFloating(sidebarActiveId) && !hostedLocation(sidebarActiveId)))
    ? sidebarActiveId.slice('terminal:'.length) : undefined;
  const activeIsDocument = activeCenterId?.startsWith('document:') ?? false;
  const activeSessionId = activeCenterId?.startsWith('session:') ? activeCenterId.slice('session:'.length) : undefined;
  const activePullRequest = openPullRequests.find(pr => (pr.repository ?? worktree.path) === worktree.path && prTabId(pr.repository ?? worktree.path, pr.number, pr) === activeCenterId);

  const linkSidebarToTab = worktree.is_main && !project.builtin;
  const sidebarDocument = documents.find(document => linkSidebarToTab
    ? `document:${document.id}` === sidebarActiveId : document.id === activeDocumentId);
  const sidebarSession = sessionPreviews.find(preview => `session:${preview.session.id}` === sidebarActiveId);
  const sidebarResourcePath = hostedTabs.find(tab => tab.id === sidebarActiveId)?.workspace
    ?? terminals.tabs.find(tab => `terminal:${tab.id}` === sidebarActiveId)?.workspace_root
    ?? sidebarDocument?.diffSource?.repository
    ?? (sidebarDocument?.explorerSource === 'notes' ? undefined : sidebarDocument?.path)
    ?? sidebarSession?.workspacePath
    ?? openPullRequests.find(pr => prTabId(pr.repository ?? worktree.path, pr.number, pr) === sidebarActiveId)?.repository;
  // Files and sessions can live below a worktree root. Prefer the closest owner.
  const sidebarWorktree = linkSidebarToTab
    ? project.worktrees.filter(item => sidebarResourcePath === item.path || sidebarResourcePath?.startsWith(`${item.path}/`))
      .sort((a, b) => b.path.length - a.path.length)[0] ?? worktree
    : worktree;
  const [sidebarWorkspaces, setSidebarWorkspaces] = useState({ files: worktree.path, git: worktree.path, sessions: worktree.path });
  const linkedRightView = rightView === 'files' || rightView === 'git' || rightView === 'sessions' ? rightView : undefined;
  useEffect(() => {
    if (!linkSidebarToTab || !active || !rightSidebarVisible || !linkedRightView) return;
    setSidebarWorkspaces(current => current[linkedRightView] === sidebarWorktree.path
      ? current : { ...current, [linkedRightView]: sidebarWorktree.path });
  }, [linkSidebarToTab, active, rightSidebarVisible, linkedRightView, sidebarWorktree.path]);
  // Only a project's main workspace follows tabs; hidden panels retain their context.
  const sidebarPath = (view: keyof typeof sidebarWorkspaces) => !linkSidebarToTab ? worktree.path
    : active && rightSidebarVisible && rightView === view ? sidebarWorktree.path : sidebarWorkspaces[view];
  const explorerRoot = sidebarPath('files');
  const gitRoot = sidebarPath('git');
  const sessionRoot = sidebarPath('sessions');

  useEffect(() => {
    if (activeCenterId && centerTabs.some((tab) => tab.id === activeCenterId)) return;
    const candidate = terminalTabs.find((tab) => tab.targetId === terminals.activeId) ?? centerTabs[0];
    setActiveCenterId(candidate?.id);
    if (candidate && (candidate.kind === 'file' || candidate.kind === 'diff')) setActiveDocumentId(candidate.targetId);
    else setActiveDocumentId(undefined);
  }, [activeCenterId, centerTabs, terminalTabs, terminals.activeId]);

  const displayedDocuments = useRef<OpenDocument[]>([]);
  useEffect(() => {
    for (const old of displayedDocuments.current) {
      const next = documents.find(item => item.instanceId === old.instanceId);
      if (next && next.id !== old.id) {
        floating.replaceDocument(worktree.path, old.id, next.id);
        void loadEditorArea().then(module => module.disposeEditorModels([old]));
        setActiveDocumentId(id => id === old.id ? next.id : id);
        setActiveCenterId(id => id === `document:${old.id}` ? `document:${next.id}` : id);
      }
    }
    displayedDocuments.current = documents;
  }, [documents]);
  useEffect(() => {
    documentsRef.current = documents;
    for (const [id, request] of documentRefreshes.current) {
      if (documents.some((item) => item.id === id && item.instanceId === request.instanceId)) continue;
      request.controller.abort();
      documentRefreshes.current.delete(id);
    }
    const previewUrls = new Set(documents.map((item) => item.imageUrl));
    for (const url of refreshedPreviewUrls.current) {
      if (previewUrls.has(url)) continue;
      revokeBlobUrl(url);
      refreshedPreviewUrls.current.delete(url);
    }
  }, [documents]);
  useEffect(() => () => {
    for (const timer of autoSaveTimers.current.values()) window.clearTimeout(timer);
    autoSaveTimers.current.clear();
    autoSavePendingAfterSave.current.clear();
    for (const request of documentRefreshes.current.values()) request.controller.abort();
    documentRefreshes.current.clear();
    for (const url of refreshedPreviewUrls.current) revokeBlobUrl(url);
    refreshedPreviewUrls.current.clear();
    for (const document of documentsRef.current) {
      revokeBlobUrl(document.imageUrl);
      void loadEditorArea().then((module) => module.disposeEditorModels([document]));
    }
  }, []);

  const activateCenter = (tab: CenterTab) => {
    if (isFloating(tab.id)) floating.add(worktree.path, tab.id);
    setActiveCenterId(tab.id);
    if ((tab.kind === 'terminal' || tab.kind === 'agent') && !hostedTab(tab.id)) terminals.activate(tab.targetId);
    else if (tab.kind === 'file' || tab.kind === 'diff') setActiveDocumentId(tab.targetId);
    else setActiveDocumentId(undefined);
  };

  const openTerminal = (tab: TerminalTab, targetFloating = openingInFloatingWorkspace()) => {
    if (tab.workspace_root !== worktree.path) {
      const descriptor = terminalCenterTab(tab, projectTerminals.agents, projectTerminals.titles);
      floating.requestTerminal(tab);
      if (targetFloating) floating.add(tab.workspace_root, descriptor.id, descriptor);
      else {
        floating.hostTerminal(tab.workspace_root, worktree.path, descriptor);
        setActiveCenterId(descriptor.id);
        setActiveDocumentId(undefined);
        setTabRevealRequest({ tabId: descriptor.id });
      }
      return;
    }
    setTerminalVisibility(items => ({ ...items, [tab.id]: true }));
    activateCenter(terminalCenterTab(tab, terminals.detectedAgents, terminals.terminalTitles));
    routeToFloating(`terminal:${tab.id}`, targetFloating);
  };

  const loadSessionSnapshot = useCallback(async (session: AowAgentSession, sessionWorkspace = worktree.path) => {
    const requestId = (sessionSnapshotRequestIds.current.get(session.id) ?? 0) + 1;
    sessionSnapshotRequestIds.current.set(session.id, requestId);
    setSessionPreviews((items) => {
      const existing = items.find((item) => item.session.id === session.id);
      return existing
        ? items.map((item) => item.session.id === session.id ? { ...item, session, workspacePath: sessionWorkspace, loading: true, error: undefined } : item)
        : [...items, { session, workspacePath: sessionWorkspace, loading: true }];
    });
    try {
      const snapshot = await sessionsApi.agentSessionSnapshot(session, sessionWorkspace);
      if (sessionSnapshotRequestIds.current.get(session.id) !== requestId) return;
      setSessionPreviews((items) => items.map((item) => item.session.id === session.id
        ? { ...item, session, snapshot, loading: false, error: undefined }
        : item));
    } catch (reason) {
      if (sessionSnapshotRequestIds.current.get(session.id) !== requestId) return;
      setSessionPreviews((items) => items.map((item) => item.session.id === session.id
        ? { ...item, loading: false, error: message(reason) }
        : item));
    }
  }, [worktree.path]);

  const openSession = useCallback((session: AowAgentSession, sessionWorkspace = session.cwd || worktree.path) => {
    setRightView('sessions');
    setActiveDocumentId(undefined);
    routeToFloating(`session:${session.id}`);
    setActiveCenterId(`session:${session.id}`);
    if (!sessionPreviews.some((item) => item.session.id === session.id)) void loadSessionSnapshot(session, sessionWorkspace);
  }, [loadSessionSnapshot, sessionPreviews]);

  const resolveAutomationSession = useCallback(async (session: AowAgentSession, reference: AutomationSessionReference) => {
    const requestId = (sessionSnapshotRequestIds.current.get(session.id) ?? 0) + 1;
    sessionSnapshotRequestIds.current.set(session.id, requestId);
    setSessionPreviews((items) => items.map((item) => item.session.id === session.id ? { ...item, loading: true, error: undefined } : item));
    try {
      const resolved = await sessionsApi.automationRunSession(reference.taskId, reference.runId);
      if (sessionSnapshotRequestIds.current.get(session.id) !== requestId) return;
      const workspacePath = resolved.cwd;
      setSessionPreviews((items) => items.map((item) => item.session.id === session.id
        ? { ...item, session: resolved, workspacePath, automationRun: undefined }
        : item));
      void loadSessionSnapshot(resolved, workspacePath);
    } catch (reason) {
      if (sessionSnapshotRequestIds.current.get(session.id) !== requestId) return;
      setSessionPreviews((items) => items.map((item) => item.session.id === session.id
        ? { ...item, loading: false, error: message(reason) }
        : item));
    }
  }, [loadSessionSnapshot]);

  const openAutomationSession = useCallback((taskId: string, run: AutomationRun) => {
    if (!run.session_id) return;
    const session: AowAgentSession = {
      id: `${run.agent}:${run.session_id}`,
      agent: run.agent,
      session_id: run.session_id,
      title: `Session ${run.session_id.slice(0, 8)}`,
      // This is only the immediately-rendered placeholder. The authoritative
      // workspace is returned by the provider lookup below; do not use the
      // automation run's workspace_path to resolve an Agent Session.
      cwd: worktree.path,
      created_at: run.started_at,
      updated_at: run.started_at,
    };
    const reference = { taskId, runId: run.id };
    setRightView('sessions');
    setActiveDocumentId(undefined);
    routeToFloating(`session:${session.id}`);
    setActiveCenterId(`session:${session.id}`);
    if (sessionPreviews.some((item) => item.session.id === session.id)) return;
    setSessionPreviews((items) => [...items, { session, workspacePath: session.cwd, automationRun: reference, loading: true }]);
    void resolveAutomationSession(session, reference);
  }, [resolveAutomationSession, sessionPreviews, worktree.path]);

  const taskChanged = useCallback((task?: AutomationTask, mutated = false) => {
    if (task) setAutomationTasks((items) => ({ ...items, [task.id]: task }));
    if (mutated) setAutomationRefreshKey((value) => value + 1);
  }, []);

  const taskDeleted = useCallback((id: string) => {
    setOpenAutomationTaskIds((items) => items.filter((taskId) => taskId !== id));
    setAutomationTasks((items) => {
      const remaining = { ...items };
      delete remaining[id];
      return remaining;
    });
    setAutomationRefreshKey((value) => value + 1);
  }, []);

  const openAutomation = useCallback((task: AutomationTask) => {
    setRightView('automations');
    setAutomationTasks((items) => ({ ...items, [task.id]: task }));
    setOpenAutomationTaskIds((items) => items.includes(task.id) ? items : [...items, task.id]);
    setActiveDocumentId(undefined);
    routeToFloating(`automation:${task.id}`);
    setActiveCenterId(`automation:${task.id}`);
  }, []);

  const createTerminal = async (agent?: AowAgent, session?: AowAgentSession, sessionWorkspace = worktree.path) => {
    const targetFloating = openingInFloatingWorkspace();
    setOperationError('');
    try {
      const tab = agent
        ? await terminalApi.create(sessionWorkspace, { cwd: session?.cwd || sessionWorkspace, agentId: agent.id, resumeSessionId: session?.session_id })
        : await terminals.create();
      if (!tab) return;
      if (tab.workspace_root !== worktree.path) { openTerminal(tab, targetFloating); return; }
      if (agent) terminals.replace(tab);
      setTerminalVisibility(items => ({ ...items, [tab.id]: true }));
      terminals.activate(tab.id);
      routeToFloating(`terminal:${tab.id}`, targetFloating);
      setActiveCenterId(`terminal:${tab.id}`);
    } catch (reason) {
      setOperationError(message(reason));
    }
  };

  const openExplorerFile = useCallback(async (entry: FileEntry, source: 'project' | 'notes' | 'external', targetFloating = openingInFloatingWorkspace()) => {
    const id = entry.path;
    setRightView('files');
    if (source === 'notes') setNotesExplorerCollapsed(false);
    else if (source === 'project') setFilesExplorerCollapsed(false);
    setActiveDocumentId(id);
    routeToFloating(`document:${id}`, targetFloating);
    setActiveCenterId(`document:${id}`);
    if (documentsRef.current.some((document) => document.id === id)) return;
    const existing = sharedDocument(id);
    if (existing) { setDocuments(items => items.some(item => item.id === id) ? items : [...items, existing]); return; }
    const kind = previewKind(entry.path);
    const instanceId = nextDocumentInstanceId();
    const base: OpenDocument = {
      id,
      path: entry.path,
      name: entry.name,
      kind,
      instanceId,
      explorerSource: source,
      readOnly: entry.readonly,
      loading: true,
      markdownView: kind === 'markdown' ? 'editor' : undefined,
    };
    if (kind === 'image' || kind === 'pdf') {
      setDocuments((items) => [...items, { ...base, readOnly: true, imageUrl: filesApi.rawUrl(entry.path), loading: false }]);
      return;
    }
    if (kind === 'unsupported') {
      setDocuments((items) => [...items, { ...base, readOnly: true, loading: false, error: '当前文件类型暂不支持预览。' }]);
      return;
    }
    setDocuments((items) => [...items, base]);
    try {
      const text = await filesApi.readText(entry.path);
      setDocuments((items) => items.map((document) => document.id === id && document.instanceId === instanceId ? {
        ...document,
        content: text.content,
        savedContent: text.content,
        version: text.version,
        language: text.language,
        mime: text.mime,
        loading: false,
      } : document));
    } catch (reason) {
      setDocuments((items) => items.map((document) => document.id === id && document.instanceId === instanceId
        ? { ...document, kind: 'unsupported', error: message(reason), loading: false }
        : document));
    }
  }, [documents]);

  const openExternalFile = useCallback(async (path: string) => {
    const targetFloating = openingInFloatingWorkspace();
    await filesApi.ensureFile(path);
    await openExplorerFile({
      name: basename(path),
      path,
      kind: 'file',
      size: 0,
      modified_ms: null,
      readonly: false,
      hidden: false,
      mode: 0,
      links: 1,
      uid: 0,
      gid: 0,
      is_symlink: false,
      link_target: null,
    }, 'external', targetFloating);
  }, [openExplorerFile]);

  const clearAutoSave = useCallback((id: string) => {
    const timer = autoSaveTimers.current.get(id);
    if (timer === undefined) return;
    window.clearTimeout(timer);
    autoSaveTimers.current.delete(id);
  }, []);

  const refreshDocument = useCallback(async (id: string) => {
    const target = documentsRef.current.find((item) => item.id === id);
    if (!target || target.loading || target.saving || savingDocumentIds.current.has(id) || documentRefreshes.current.has(id)) return;
    clearAutoSave(id);
    const controller = new AbortController();
    const request = { instanceId: target.instanceId, controller };
    documentRefreshes.current.set(id, request);
    const matches = (item: OpenDocument) => item.id === id && item.instanceId === target.instanceId;
    const current = () => !controller.signal.aborted && documentsRef.current.some(matches);
    setDocuments((items) => items.map((item) => matches(item) ? { ...item, refreshing: true, refreshError: undefined } : item));
    try {
      const kind = target.diffSource ? 'diff' : previewKind(target.path);
      if (target.diffSource) {
        const diff = await readDocumentDiff(target, controller.signal);
        if (!current()) return;
        setDocuments((items) => items.map((item) => matches(item) ? { ...item, ...diff, refreshing: false, error: undefined } : item));
      } else if (kind === 'image' || kind === 'pdf') {
        const blob = await filesApi.readBlob(target.path, controller.signal);
        if (!current()) return;
        const imageUrl = URL.createObjectURL(blob);
        refreshedPreviewUrls.current.add(imageUrl);
        setDocuments((items) => items.map((item) => matches(item) ? { ...item, kind, imageUrl, refreshing: false, error: undefined } : item));
      } else if (kind === 'unsupported') {
        throw new Error('当前文件类型暂不支持预览。');
      } else {
        const text = await filesApi.readText(target.path, controller.signal);
        if (!current()) return;
        setDocuments((items) => items.map((item) => {
          if (!matches(item)) return item;
          // Reconcile against the latest buffer, including edits made during the read.
          if (item.dirty && item.content !== text.content) {
            return text.content === item.savedContent
              ? { ...item, refreshing: false, version: text.version, pendingExternal: undefined }
              : { ...item, refreshing: false, pendingExternal: text };
          }
          return { ...item, kind, content: text.content, savedContent: text.content, version: text.version,
            language: text.language, mime: text.mime, dirty: false, pendingExternal: undefined, refreshing: false, error: undefined };
        }));
      }
    } catch (reason) {
      if (current()) setDocuments((items) => items.map((item) => matches(item)
        ? { ...item, refreshing: false, refreshError: message(reason) } : item));
    } finally {
      if (documentRefreshes.current.get(id) === request) documentRefreshes.current.delete(id);
    }
  }, [clearAutoSave]);

  const saveDocument = useCallback(async (id: string) => {
    const target = sharedDocument(id);
    if (!target || !canAutoSaveDocument(target) || savingDocumentIds.current.has(id) || documentRefreshes.current.has(id)) return;
    clearAutoSave(id);
    savingDocumentIds.current.add(id);
    const submittedContent = target.content;
    updateSharedDocument(target, item => ({ ...item, saving: true }));
    try {
      const result = await filesApi.saveText(target.path, submittedContent, target.version);
      autoSaveFailedIds.current.delete(id);
      updateSharedDocument(target, item => ({
        ...item,
        saving: false,
        savedContent: submittedContent,
        dirty: item.content !== submittedContent,
        version: item.path === target.path ? result.version : item.version,
        pendingExternal: undefined,
        refreshError: undefined,
      }));
      if (target.explorerSource === 'notes') onNotesChanged(dirname(target.path));
      else {
        setExplorerRefresh((current) => ({ generation: current.generation + 1, directory: dirname(target.path) }));
        setSourceControlRefresh((current) => current + 1);
      }
    } catch (reason) {
      if (autoSavePendingAfterSave.current.has(id)) autoSaveFailedIds.current.delete(id);
      else autoSaveFailedIds.current.add(id);
      updateSharedDocument(target, item => ({ ...item, saving: false }));
      setOperationError(`${target.explorerSource === 'notes' ? 'Notes' : '文件'}保存失败：${message(reason)}`);
    } finally {
      savingDocumentIds.current.delete(id);
      autoSavePendingAfterSave.current.delete(id);
    }
  }, [clearAutoSave, onNotesChanged]);

  const queueAutoSave = useCallback((id: string) => {
    clearAutoSave(id);
    autoSaveTimers.current.set(id, window.setTimeout(() => {
      autoSaveTimers.current.delete(id);
      void saveDocument(id);
    }, autoSaveDelayMs));
  }, [clearAutoSave, saveDocument]);

  const changeDocument = (id: string, content: string) => {
    const target = documentsRef.current.find((document) => document.id === id);
    if (!target) return;
    if (savingDocumentIds.current.has(id)) autoSavePendingAfterSave.current.add(id);
    const next = { ...target, content, dirty: content !== target.savedContent };
    setDocuments((items) => items.map((item) => item.id === id
      ? { ...item, content, dirty: content !== item.savedContent, editRevision: (item.editRevision ?? 0) + 1 }
      : item));
    if (canAutoSaveDocument(next)) {
      autoSaveFailedIds.current.delete(id);
      queueAutoSave(id);
    } else {
      clearAutoSave(id);
    }
  };

  useEffect(() => {
    const eligibleDocumentIds = new Set<string>();
    for (const document of documents) {
      if (!canAutoSaveDocument(document)) continue;
      eligibleDocumentIds.add(document.id);
      if (!autoSaveTimers.current.has(document.id) && !autoSaveFailedIds.current.has(document.id)) {
        queueAutoSave(document.id);
      }
    }
    for (const [id, timer] of autoSaveTimers.current) {
      if (eligibleDocumentIds.has(id)) continue;
      window.clearTimeout(timer);
      autoSaveTimers.current.delete(id);
    }
  }, [documents, queueAutoSave]);

  const reloadExternalDocument = (id: string) => {
    clearAutoSave(id);
    autoSaveFailedIds.current.delete(id);
    autoSavePendingAfterSave.current.delete(id);
    setDocuments((items) => items.map((item) => {
      if (item.id !== id || !item.pendingExternal) return item;
      const text = item.pendingExternal;
      return {
        ...item,
        content: text.content,
        savedContent: text.content,
        version: text.version,
        language: text.language,
        mime: text.mime,
        dirty: false,
        pendingExternal: undefined,
        refreshError: undefined,
      };
    }));
  };

  const keepLocalDocument = (id: string) => {
    const target = documents.find((item) => item.id === id);
    if (!target?.pendingExternal) return;
    if (!window.confirm('下次保存会用当前编辑内容覆盖磁盘上的新版本，确定继续吗？')) return;
    clearAutoSave(id);
    autoSaveFailedIds.current.delete(id);
    autoSavePendingAfterSave.current.delete(id);
    setDocuments((items) => items.map((item) => item.id === id && item.pendingExternal ? {
      ...item,
      savedContent: item.pendingExternal.content,
      version: item.pendingExternal.version,
      dirty: item.content !== item.pendingExternal.content,
      pendingExternal: undefined,
      refreshError: undefined,
    } : item));
  };

  const createTemporaryNote = async (kind: 'md' | 'txt') => {
    const targetFloating = openingInFloatingWorkspace();
    setOperationError('');
    try {
      const created = await aowApi.createTemporaryNote(project.id, kind);
      onNotesChanged(project.notes_path);
      await openExplorerFile({
        name: created.name,
        path: created.path,
        kind: 'file',
        size: 0,
        modified_ms: null,
        readonly: false,
        hidden: true,
        mode: 0,
        links: 1,
        uid: 0,
        gid: 0,
        is_symlink: false,
        link_target: null,
      }, 'notes', targetFloating);
    } catch (reason) {
      setOperationError(message(reason));
    }
  };

  const createExplorerEntry = async (source: 'project' | 'notes', parent: string, name: string, kind: 'file' | 'directory') => {
    setOperationError('');
    const created = await filesApi.createEntry(parent, name, kind);
    if (source === 'notes') onNotesChanged(parent);
    else {
      setExplorerRefresh((current) => ({ generation: current.generation + 1, directory: parent }));
      setSourceControlRefresh((current) => current + 1);
    }
    if (kind === 'file') {
      await openExplorerFile({
        name: created.name,
        path: created.path,
        kind: 'file',
        size: 0,
        modified_ms: null,
        readonly: false,
        hidden: name.startsWith('.'),
        mode: 0,
        links: 1,
        uid: 0,
        gid: 0,
        is_symlink: false,
        link_target: null,
      }, source);
    }
    return created;
  };

  const deleteExplorerEntry = async (source: 'project' | 'notes', path: string, directory: boolean) => {
    const label = basename(path);
    const affectedDocuments = documents.filter((document) => document.explorerSource === source
      && (document.path === path || (directory && document.path.startsWith(`${path}/`))));
    const dirtyDocuments = affectedDocuments.filter((document) => document.dirty);
    if (dirtyDocuments.length && !window.confirm(`${dirtyDocuments.length} 个将被删除的文件包含未保存更改。确定继续吗？`)) return;
    const location = source === 'notes' ? 'Notes' : 'Project';
    if (!window.confirm(`删除 ${location} 中的${directory ? '目录及其全部内容' : '文件'}“${label}”？`)) return;
    setOperationError('');
    try {
      await filesApi.deleteEntry(path);
      const removedIds = affectedDocuments.map((document) => document.id);
      const removedIdSet = new Set(removedIds);
      for (const document of affectedDocuments) {
        clearAutoSave(document.id);
        autoSaveFailedIds.current.delete(document.id);
        autoSavePendingAfterSave.current.delete(document.id);
        revokeBlobUrl(document.imageUrl);
      }
      setDocuments((items) => items.filter((document) => !removedIdSet.has(document.id)));
      setActiveDocumentId((current) => current && removedIdSet.has(current) ? undefined : current);
      setActiveCenterId((current) => current && removedIds.some((id) => current === `document:${id}`) ? undefined : current);
      if (source === 'notes') onNotesChanged(dirname(path));
      else {
        setExplorerRefresh((current) => ({ generation: current.generation + 1, directory: dirname(path) }));
        setSourceControlRefresh((current) => current + 1);
      }
    } catch (reason) {
      setOperationError(message(reason));
    }
  };

  const renameExplorerEntry = async (source: 'project' | 'notes', path: string, requestedName: string) => {
    const name = requestedName.trim();
    if (!name || name === '.' || name === '..' || /[\/\\\0]/.test(name) || new TextEncoder().encode(name).length > 255) {
      throw new Error('名称无效：请输入不含路径分隔符且不超过 255 字节的名称。');
    }
    const existingDocument = documents.find((document) => document.explorerSource === source && document.path === path);
    if (existingDocument) return renameAowFile(existingDocument.id, name);
    if (documents.some((document) => document.explorerSource === source && document.path.startsWith(`${path}/`))) {
      throw new Error('该目录中有文件正在工作区打开，请先关闭对应 Tab。');
    }
    const result = await filesApi.renameEntry(path, name);
    if (source === 'notes') onNotesChanged(dirname(path));
    else {
      setExplorerRefresh((current) => ({ generation: current.generation + 1, directory: dirname(path) }));
      setSourceControlRefresh((current) => current + 1);
    }
    return result;
  };

  const openDocumentDiff = useCallback(async (base: OpenDocument) => {
    const id = base.id;
    setRightView('git');
    setActiveDocumentId(id);
    routeToFloating(`document:${id}`);
    setActiveCenterId(`document:${id}`);
    if (documentsRef.current.some(document => document.id === id)) return;
    const document = { ...base, instanceId: nextDocumentInstanceId() };
    setDocuments(items => [...items, document]);
    try {
      const diff = await readDocumentDiff(document);
      setDocuments(items => items.map(item => item.id === id && item.instanceId === document.instanceId ? { ...item, ...diff, loading: false } : item));
    } catch (reason) {
      setDocuments(items => items.map(item => item.id === id && item.instanceId === document.instanceId ? { ...item, loading: false, error: message(reason) } : item));
    }
  }, []);
  const openDiff = useCallback((repo: string, file: GitFileStatus, staged: boolean) => openDocumentDiff(diffDocument({
    type: 'diff', workspace: worktree.id, repository: repo, path: file.path, source: staged ? 'staged' : 'working', untracked: file.worktree_status === '?',
  })), [openDocumentDiff]);
  const openCommitDiff = useCallback((repo: string, commit: GitCommit, file: GitCommitFile) => openDocumentDiff(diffDocument({
    type: 'diff', workspace: worktree.id, repository: repo, path: file.path, source: 'commit', commit: commit.id, originalPath: file.original_path ?? undefined,
  })), [openDocumentDiff]);

  const openPullRequest = useCallback((value: PullRequestSummary, repository = worktree.path) => {
    const pr = { ...value, repository };
    setOpenPullRequests((items) => {
      const index = items.findIndex(item => prTabId(item.repository ?? worktree.path, item.number, item) === prTabId(repository, pr.number, pr));
      if (index < 0) return [...items, pr];
      return items.map(item => prTabId(item.repository ?? worktree.path, item.number, item) === prTabId(repository, pr.number, pr) ? pr : item);
    });
    setRightView('pullRequests');
    setActiveDocumentId(undefined);
    routeToFloating(prTabId(repository, pr.number, pr));
    setActiveCenterId(prTabId(repository, pr.number, pr));
  }, []);

  const closeDocument = (id: string) => {
    const closing = documents.find((document) => document.id === id);
    if (closing?.dirty && !window.confirm(`“${closing.name}”有未保存的更改，确定关闭吗？`)) return false;
    clearAutoSave(id);
    autoSaveFailedIds.current.delete(id);
    autoSavePendingAfterSave.current.delete(id);
    revokeBlobUrl(closing?.imageUrl);
    if (closing) void loadEditorArea().then((module) => module.disposeEditorModels([closing]));
    const closingTabId = `document:${id}`;
    if (activeCenterId === closingTabId) {
      const closingIndex = centerTabs.findIndex((tab) => tab.id === closingTabId);
      const candidate = centerTabs.slice(closingIndex + 1).find((tab) => tab.id !== closingTabId)
        ?? centerTabs.slice(0, Math.max(closingIndex, 0)).reverse().find((tab) => tab.id !== closingTabId);
      setActiveCenterId(candidate?.id);
      if (candidate && centerTabGroup(candidate) === 'terminal') terminals.activate(candidate.targetId);
      setActiveDocumentId(candidate && (candidate.kind === 'file' || candidate.kind === 'diff') ? candidate.targetId : undefined);
    } else {
      setActiveDocumentId((current) => current === id ? undefined : current);
    }
    setDocuments((items) => items.filter((document) => document.id !== id));
    return true;
  };

  const closeSession = (id: string) => {
    const closingTabId = `session:${id}`;
    if (activeCenterId === closingTabId) {
      const closingIndex = centerTabs.findIndex((tab) => tab.id === closingTabId);
      const candidate = centerTabs.slice(closingIndex + 1).find((tab) => tab.id !== closingTabId)
        ?? centerTabs.slice(0, Math.max(closingIndex, 0)).reverse().find((tab) => tab.id !== closingTabId);
      setActiveCenterId(candidate?.id);
      if (candidate && centerTabGroup(candidate) === 'terminal') terminals.activate(candidate.targetId);
      setActiveDocumentId(candidate && (candidate.kind === 'file' || candidate.kind === 'diff') ? candidate.targetId : undefined);
    }
    setSessionPreviews((items) => items.filter((item) => item.session.id !== id));
  };

  const closeAutomation = (id: string) => {
    const closingTabId = `automation:${id}`;
    if (activeCenterId === closingTabId) {
      const closingIndex = centerTabs.findIndex((tab) => tab.id === closingTabId);
      const candidate = centerTabs.slice(closingIndex + 1).find((tab) => tab.id !== closingTabId)
        ?? centerTabs.slice(0, Math.max(closingIndex, 0)).reverse().find((tab) => tab.id !== closingTabId);
      setActiveCenterId(candidate?.id);
      if (candidate && centerTabGroup(candidate) === 'terminal') terminals.activate(candidate.targetId);
      setActiveDocumentId(candidate && (candidate.kind === 'file' || candidate.kind === 'diff') ? candidate.targetId : undefined);
    }
    setOpenAutomationTaskIds((items) => items.filter((taskId) => taskId !== id));
    setAutomationTasks((items) => {
      const remaining = { ...items };
      delete remaining[id];
      return remaining;
    });
  };

  const closePullRequest = (closingTabId: string) => {
    if (activeCenterId === closingTabId) {
      const closingIndex = centerTabs.findIndex((tab) => tab.id === closingTabId);
      const candidate = centerTabs.slice(closingIndex + 1).find((tab) => tab.id !== closingTabId)
        ?? centerTabs.slice(0, Math.max(closingIndex, 0)).reverse().find((tab) => tab.id !== closingTabId);
      setActiveCenterId(candidate?.id);
      if (candidate && centerTabGroup(candidate) === 'terminal') terminals.activate(candidate.targetId);
      setActiveDocumentId(candidate && (candidate.kind === 'file' || candidate.kind === 'diff') ? candidate.targetId : undefined);
    }
    setOpenPullRequests(items => items.filter(pr => prTabId(pr.repository ?? worktree.path, pr.number, pr) !== closingTabId));
  };

  const hideTerminalTabs = (targets: CenterTab[]) => {
    const closingIds = new Set(targets.map(tab => tab.id));
    const localTargets = targets.filter(tab => !hostedTab(tab.id));
    setTerminalVisibility(items => ({ ...items, ...Object.fromEntries(localTargets.map(tab => [tab.targetId, false])) }));
    for (const tab of targets) {
      const hosted = hostedTab(tab.id);
      if (hosted) floating.sources.current.get(hosted.workspace)?.close(tab.id);
      floating.cancelTerminalRequest(tab.targetId);
      floating.remove(hosted?.workspace ?? worktree.path, tab.id);
      floating.removeHosted(hosted?.workspace ?? worktree.path, tab.id);
    }
    if (activeCenterId && closingIds.has(activeCenterId)) {
      const index = centerTabs.findIndex(tab => tab.id === activeCenterId);
      const candidate = centerTabs.slice(index + 1).find(tab => !closingIds.has(tab.id))
        ?? centerTabs.slice(0, Math.max(index, 0)).reverse().find(tab => !closingIds.has(tab.id));
      setActiveCenterId(candidate?.id);
      if (candidate && centerTabGroup(candidate) === 'terminal') terminals.activate(candidate.targetId);
      setActiveDocumentId(candidate && (candidate.kind === 'file' || candidate.kind === 'diff') ? candidate.targetId : undefined);
    }
  };

  const closeTab = async (tab: CenterTab) => {
    if (tab.kind === 'browser') { setBrowserTab(false); floating.remove(worktree.path, tab.id); return; }
    if (centerTabGroup(tab) === 'terminal') {
      hideTerminalTabs([tab]);
    }
    else if (tab.kind === 'session') closeSession(tab.targetId);
    else if (tab.kind === 'automation') closeAutomation(tab.targetId);
    else if (tab.kind === 'pullRequest') closePullRequest(tab.targetId);
    else closeDocument(tab.targetId);
  };

  const closeTabsInGroup = async (selected: CenterTab, ids: string[]) => {
    const group = centerTabGroup(selected);
    const targets = centerTabs.filter(tab => centerTabGroup(tab) === group && ids.includes(tab.id));
    if (!targets.length) return;
    if (group === 'terminal') {
      hideTerminalTabs(targets);
      return;
    }
    if (group === 'browser') {
      for (const tab of targets) await closeTab(tab);
      return;
    }
    const targetTabIds = new Set(targets.map((tab) => tab.id));
    const targetIds = new Set(targets.map((tab) => tab.targetId));
    for (const document of documents) {
      if (targetIds.has(document.id)) {
        clearAutoSave(document.id);
        autoSaveFailedIds.current.delete(document.id);
        autoSavePendingAfterSave.current.delete(document.id);
        revokeBlobUrl(document.imageUrl);
        void loadEditorArea().then((module) => module.disposeEditorModels([document]));
      }
    }
    if (activeCenterId && targetTabIds.has(activeCenterId)) {
      const selectedIndex = centerTabs.findIndex((tab) => tab.id === selected.id);
      const candidate = centerTabs.slice(selectedIndex + 1).find((tab) => !targetTabIds.has(tab.id))
        ?? centerTabs.slice(0, Math.max(selectedIndex, 0)).reverse().find((tab) => !targetTabIds.has(tab.id));
      setActiveCenterId(candidate?.id);
      if (candidate && centerTabGroup(candidate) === 'terminal') terminals.activate(candidate.targetId);
      setActiveDocumentId(candidate && (candidate.kind === 'file' || candidate.kind === 'diff') ? candidate.targetId : undefined);
    } else {
      setActiveDocumentId((current) => current && targetIds.has(current) ? undefined : current);
    }
    if (group === 'session') setSessionPreviews((items) => items.filter((item) => !targetIds.has(item.session.id)));
    else if (group === 'automation') {
      setOpenAutomationTaskIds((items) => items.filter((taskId) => !targetIds.has(taskId)));
      setAutomationTasks((items) => Object.fromEntries(Object.entries(items).filter(([taskId]) => !targetIds.has(taskId))));
    } else if (group === 'pullRequest') setOpenPullRequests((items) => items.filter((pr) => !targetIds.has(prTabId(pr.repository ?? worktree.path, pr.number, pr))));
    else setDocuments((items) => items.filter((document) => !targetIds.has(document.id)));
  };

  const canRenameTab = (tab: CenterTab) => {
    const hosted = hostedTab(tab.id);
    if (hosted) return floating.sources.current.get(hosted.workspace)?.canRename(tab.id) ?? false;
    if (tab.renameable === false || (tab.kind !== 'terminal' && tab.kind !== 'agent' && tab.kind !== 'file')) return false;
    if (tab.kind === 'file') {
      const document = documents.find(item => item.id === tab.targetId);
      if (!document || document.loading) {
        setOperationError('文件仍在加载，请稍后再重命名。');
        return false;
      }
      if (document.dirty) {
        setOperationError('文件有未保存的更改，请先保存后再重命名。');
        return false;
      }
    }
    setOperationError('');
    return true;
  };

  const renameAowFile = async (oldPath: string, requestedName: string) => {
    const name = requestedName.trim();
    if (!name || name === '.' || name === '..' || name.includes('/') || name.includes(String.fromCharCode(92))
      || name.includes(String.fromCharCode(0)) || new TextEncoder().encode(name).length > 255) {
      throw new Error('文件名无效：请输入不含路径分隔符且不超过 255 字节的单个文件名。');
    }

    const document = documents.find((item) => item.id === oldPath);
    if (document?.loading) throw new Error('文件仍在加载，请稍后再重命名。');
    if (document?.dirty) throw new Error('文件有未保存的更改，请先保存后再重命名。');

    const expectedPath = joinPath(dirname(oldPath), name);
    if (documents.some((item) => item.id !== oldPath && item.path === expectedPath)) {
      throw new Error('该文件已在工作区中打开，请先关闭对应 Tab。');
    }

    const result = await filesApi.renameEntry(oldPath, name);
    const nextId = result.path;
    if (document) {
      const kind = previewKind(result.path);
      let text: Awaited<ReturnType<typeof filesApi.readText>> | undefined;
      let readError = '';
      if (kind === 'text' || kind === 'json' || kind === 'markdown') {
        try {
          text = await filesApi.readText(result.path);
        } catch (reason) {
          readError = message(reason);
        }
      }
      setDocuments((items) => items.map((item) => item.id === oldPath ? {
        ...item,
        id: nextId,
        path: result.path,
        name: result.name,
        kind: readError ? 'unsupported' : kind,
        language: text?.language ?? editorLanguage(result.path),
        mime: text?.mime,
        content: text?.content,
        savedContent: text?.content,
        version: text?.version,
        dirty: false,
        loading: false,
        imageUrl: kind === 'image' || kind === 'pdf'
          ? filesApi.rawUrl(result.path)
          : undefined,
        error: readError || (kind === 'unsupported' ? '当前文件类型暂不支持预览。' : undefined),
        originalContent: undefined,
        pendingExternal: undefined,
        refreshError: undefined,
      } : item));
      if (kind !== 'image' && kind !== 'pdf') revokeBlobUrl(document.imageUrl);
      setActiveDocumentId((current) => current === oldPath ? nextId : current);
      setActiveCenterId((current) => current === 'document:' + oldPath ? 'document:' + nextId : current);
      void loadEditorArea().then((module) => module.disposeEditorModels([document]));
    }
    if (document?.explorerSource === 'notes') {
      onNotesChanged(dirname(oldPath));
    } else {
      setExplorerRefresh((current) => ({ generation: current.generation + 1, directory: dirname(oldPath) }));
      setSourceControlRefresh((current) => current + 1);
    }
    return { ...result, path: nextId };
  };

  const renameTab = async (tab: CenterTab, requestedName: string) => {
    const hosted = hostedTab(tab.id);
    if (hosted) {
      await floating.sources.current.get(hosted.workspace)?.rename(tab.id, requestedName);
      return;
    }
    if (tab.renameable === false) return;
    const name = requestedName.trim();
    if (!name) throw new Error('名称不能为空。');
    if (tab.kind === 'terminal' || tab.kind === 'agent') {
      await terminals.rename(tab.targetId, name);
      return;
    }
    if (name === tab.label || tab.kind !== 'file') return;
    setOperationError('');
    try {
      await renameAowFile(tab.targetId, name);
    } catch (reason) {
      throw new Error(`文件重命名失败：${message(reason)}`);
    }
  };

  const gitWorktree = project.worktrees.find(item => item.path === gitRoot) ?? worktree;
  const repository = useMemo<RepositorySummary>(() => ({
    path: gitRoot, name: project.name, branch: gitWorktree.detached ? 'detached' : gitWorktree.branch,
  }), [project.name, gitWorktree.branch, gitWorktree.detached, gitRoot]);
  const activeNotePath = sidebarDocument?.explorerSource === 'notes' ? sidebarDocument.path : undefined;

  useEffect(() => {
    let cancelled = false;
    restoring.current = true;
    const saved = readStored<{ documents?: OpenDocument[]; sessions?: SessionPreview[]; pullRequests?: (PullRequestSummary & { repository?: string })[]; tasks?: AutomationTask[]; terminalVisibility?: Record<string, boolean>; openedCliTerminals?: string[]; active?: string }>(`aow-workspace-tabs:${worktree.path}`, {});
    setTerminalVisibility(saved.terminalVisibility ?? Object.fromEntries((saved.openedCliTerminals ?? []).filter(id => typeof id === 'string').map(id => [id, true])));
    const pending = (Array.isArray(saved.documents) ? saved.documents : []).filter(item => typeof item.path === 'string' && item.path.length < 8192).map(async item => {
      if (item.diffSource) {
        const document = { ...item, content: undefined, savedContent: undefined, loading: true, instanceId: nextDocumentInstanceId() };
        setDocuments(items => items.some(value => value.id === document.id) ? items : [...items, document]);
        try {
          const diff = await readDocumentDiff(document);
          if (!cancelled) setDocuments(items => items.map(value => value.id === document.id ? { ...value, ...diff, loading: false } : value));
        } catch (reason) { if (!cancelled) setDocuments(items => items.map(value => value.id === document.id ? { ...value, loading: false, error: message(reason) } : value)); }
      } else {
        await openExplorerFile({ name: basename(item.path), path: item.path, kind: 'file', readonly: item.readOnly ?? false,
          size: 0, modified_ms: null, hidden: false, mode: 0, links: 1, uid: 0, gid: 0, is_symlink: false, link_target: null }, item.explorerSource ?? 'external');
      }
    });
    for (const preview of saved.sessions ?? []) {
      if (preview.session?.id && preview.workspacePath) {
        if (preview.automationRun) {
          setSessionPreviews(items => [...items, { ...preview, loading: true }]);
          void resolveAutomationSession(preview.session, preview.automationRun);
        } else openSession(preview.session, preview.workspacePath);
      }
    }
    for (const pr of saved.pullRequests ?? []) if (typeof pr.number === 'number') openPullRequest(pr, pr.repository ?? worktree.path);
    for (const task of saved.tasks ?? []) if (typeof task.id === 'string') openAutomation(task);
    restoring.current = false;
    void Promise.allSettled(pending).then(() => {
      if (cancelled) return;
      if (saved.active) setActiveCenterId(/^pull-request:\d+$/.test(saved.active) ? prTabId(worktree.path, Number(saved.active.slice('pull-request:'.length))) : saved.active);
      setRestored(true);
    });
    return () => { cancelled = true; };
  }, []);
  useEffect(() => {
    if (!restored) return;
    persist(`aow-workspace-tabs:${worktree.path}`, {
      documents: documents.map(({ id, path, name, kind, explorerSource, diffSource, readOnly }) => ({ id, path, name, kind, explorerSource, diffSource, readOnly })),
      sessions: sessionPreviews.map(({ session, workspacePath, automationRun }) => ({ session, workspacePath, automationRun })),
      terminalVisibility,
      pullRequests: openPullRequests, tasks: Object.values(automationTasks), active: activeCenterId,
    });
  }, [restored, documents, sessionPreviews, openPullRequests, automationTasks, activeCenterId, worktree.path, terminalVisibility]);

  const routeActions = useRef<TabOpenActions>(null!);
  routeActions.current = {
    terminal: id => {
      if (!terminals.tabs.some(tab => tab.id === id)) throw new Error('该 Tab 已关闭或不存在。');
      setTerminalVisibility(items => ({ ...items, [id]: true }));
      setActiveCenterId(`terminal:${id}`); setActiveDocumentId(undefined); terminals.activate(id); routeToFloating(`terminal:${id}`);
    },
    files: path => { setBrowserRequest({ path }); setBrowserOpened(true); setBrowserTab(true); setActiveCenterId('system-files'); routeToFloating('system-files', true); },
    file: (path, source) => openExplorerFile({ name: basename(path), path, kind: 'file', readonly: false, size: 0, modified_ms: null, hidden: false, mode: 0, links: 1, uid: 0, gid: 0, is_symlink: false, link_target: null }, source),
    diff: target => openDocumentDiff(diffDocument(target)),
    pullRequest: openPullRequest,
    session: openSession,
    automation: (task, location) => { setAutomationLocations(items => ({ ...items, [task.id]: location })); openAutomation(task); },
  };
  useEffect(() => {
    if (!restored || !terminals.loaded || terminals.loading) return;
    // A global list can discover a CLI terminal before its owner's next poll.
    for (const tab of floating.terminalRequests) {
      if (tab.workspace_root !== worktree.path) continue;
      if (!terminals.tabs.some(item => item.id === tab.id)) terminals.replace(tab);
      setTerminalVisibility(items => ({ ...items, [tab.id]: true }));
      setActiveCenterId(`terminal:${tab.id}`);
      setActiveDocumentId(undefined);
      floating.acknowledgeTerminal(tab);
    }
  }, [restored, terminals.loaded, terminals.loading, terminals.tabs, terminals.replace,
    floating.terminalRequests, floating.acknowledgeTerminal, worktree.path]);
  useEffect(() => {
    if (!restored || !terminals.loaded || terminals.loading || terminals.error) return;
    for (const tab of floating.hostedTabs) {
      if (tab.workspace !== worktree.path || terminals.tabs.some(item => item.id === tab.targetId)
        || floating.terminalRequests.some(item => item.id === tab.targetId) || initialEntry?.terminal?.id === tab.targetId) continue;
      floating.removeHosted(worktree.path, tab.id);
    }
  }, [restored, terminals.loaded, terminals.loading, terminals.error, terminals.tabs, initialEntry,
    floating.hostedTabs, floating.terminalRequests, floating.removeHosted, worktree.path]);
  useEffect(() => {
    if (!initialEntry || initialEntryApplied.current === initialEntry || !restored || !terminals.loaded || terminals.loading) return;
    const hosted = hostedTabs.find(tab => tab.id === tabCenterId(initialEntry.target) && tab.workspace === initialEntry.workspacePath);
    if (hosted) {
      initialEntryApplied.current = initialEntry;
      setActiveCenterId(hosted.id);
      setActiveDocumentId(undefined);
      setTabRevealRequest({ tabId: hosted.id });
      return;
    }
    // A CLI-created tab may appear in the global list before its owner's next poll.
    if (initialEntry.terminal && !terminals.tabs.some(tab => tab.id === initialEntry.terminal!.id)) {
      terminals.replace(initialEntry.terminal);
      return;
    }
    initialEntryApplied.current = initialEntry;
    void Promise.resolve().then(() => openTabTarget(initialEntry, routeActions.current)).catch(reason => reportTabLocationError(message(reason)));
  }, [initialEntry, restored, terminals.loaded, terminals.loading, terminals.tabs, terminals.replace, hostedTabs, reportTabLocationError]);
  const locationCenterId = locationSource === 'floating' ? floatingId : activeCenterId;
  // The center can retain a floated tab's placeholder while its content is hidden.
  const locationTabVisible = locationSource === 'floating' || (!isFloating(locationCenterId ?? '') && !hostedLocation(locationCenterId ?? ''));
  useEffect(() => {
    if (!locationSource || !restored || !terminals.loaded || terminals.loading) return;
    const tab = centerTabs.find(tab => tab.id === locationCenterId);
    if (locationCenterId && !tab) return;
    const target = captureTabTarget({ workspace: worktree.id, workspacePath: worktree.path, tab, documents,
      sessions: sessionPreviews, pullRequests: openPullRequests, browserPath, automationLocations });
    // A browser or async resource may still be acquiring its identity.
    if (tab && !target) return;
    syncTabLocation(target, '', locationTabVisible);
  }, [initialEntry, locationSource, locationCenterId, locationTabVisible, restored, terminals.loaded, terminals.loading, centerTabs,
    documents, sessionPreviews, openPullRequests, browserPath, automationLocations, worktree.id, worktree.path, syncTabLocation]);
  useEffect(() => {
    if (!floating.notesMoves.length) return;
    const remap = (path: string) => {
      const move = floating.notesMoves.find(move => path.startsWith(move.from + '/'));
      return move ? move.to + path.slice(move.from.length) : path;
    };
    const targets = documentsRef.current.filter(item => !item.diffSource && remap(item.path) !== item.path);
    if (!targets.length) return;
    setDocuments(items => items.map(item => !item.diffSource && remap(item.path) !== item.path
      ? { ...item, id: remap(item.id), path: remap(item.path), refreshing: true, refreshError: undefined } : item));
    // A cross-filesystem move changes file versions. Reconcile them before auto-save resumes.
    for (const target of targets) void filesApi.readText(remap(target.path)).then(text => {
      updateSharedDocument(target, item => {
        if (item.path !== remap(target.path)) return item;
        const conflict = item.dirty && text.content !== item.content && text.content !== item.savedContent;
        return { ...item, refreshing: false, version: text.version, pendingExternal: conflict ? text : undefined,
          savedContent: conflict ? item.savedContent : text.content,
          content: item.dirty ? item.content : text.content, dirty: item.dirty && item.content !== text.content };
      });
    }).catch(reason => updateSharedDocument(target, item => ({ ...item, refreshing: false, refreshError: message(reason) })));
  }, [floating.notesMoves]);

  const commandsRef = useRef({ createTerminal, createTemporaryNote, closeTab, closeTabsInGroup, centerTabs, canRenameTab, renameTab });
  commandsRef.current = { createTerminal, createTemporaryNote, closeTab, closeTabsInGroup, centerTabs, canRenameTab, renameTab };
  useEffect(() => {
    floating.publish(worktree.path, {
      tabs: ownedCenterTabs,
      reveal: id => setTabRevealRequest({ tabId: id }),
      create: (kind, agent) => {
        if (kind === 'files') { setBrowserOpened(true); setBrowserTab(true); floating.add(worktree.path, 'system-files'); }
        else if (kind === 'markdown' || kind === 'text') void commandsRef.current.createTemporaryNote(kind === 'markdown' ? 'md' : 'txt');
        else void commandsRef.current.createTerminal(agent);
      },
      close: id => {
        const tab = commandsRef.current.centerTabs.find(tab => tab.id === id);
        if (tab) void commandsRef.current.closeTab(tab);
        else {
          if (id.startsWith('terminal:')) {
            const terminalId = id.slice('terminal:'.length);
            floating.cancelTerminalRequest(terminalId);
            setTerminalVisibility(items => ({ ...items, [terminalId]: false }));
          }
          floating.remove(worktree.path, id);
          floating.removeHosted(worktree.path, id);
        }
      },
      closeMany: async ids => {
        const tab = commandsRef.current.centerTabs.find(tab => ids.includes(tab.id));
        if (tab) await commandsRef.current.closeTabsInGroup(tab, ids);
      },
      canRename: id => {
        const tab = commandsRef.current.centerTabs.find(tab => tab.id === id);
        return Boolean(tab && commandsRef.current.canRenameTab(tab));
      },
      rename: async (id, name) => {
        const tab = commandsRef.current.centerTabs.find(tab => tab.id === id);
        if (tab) await commandsRef.current.renameTab(tab, name);
      },
    });
  }, [ownedCenterTabs, floating.publish, worktree.path]);
  useEffect(() => {
    if (project.builtin && restored && !terminals.loading) floating.restore(worktree.path, centerTabs);
  }, [project.builtin, restored, terminals.loading, centerTabs, floating.restore, worktree.path]);
  useEffect(() => () => { floating.sources.current.delete(worktree.path); }, [worktree.path]);
  const previousTabs = useRef<CenterTab[]>([]);
  useEffect(() => {
    for (const tab of previousTabs.current) if (!ownedCenterTabs.some(item => item.id === tab.id)) {
      floating.remove(worktree.path, tab.id);
      floating.removeHosted(worktree.path, tab.id);
    }
    previousTabs.current = ownedCenterTabs;
  }, [ownedCenterTabs, floating.remove, floating.removeHosted, worktree.path]);
  const renderEditor = (selectedDocumentId?: string) => (
          <Suspense fallback={<div className="project-aow-empty">正在加载编辑器…</div>}>
            <EditorArea
              documents={documents}
              activeId={selectedDocumentId}
              wordWrapOverride={wordWrapFor(selectedDocumentId)}
              onWordWrapChange={setWordWrap}
              onActivate={(id) => { setActiveDocumentId(id); setActiveCenterId(`document:${id}`); }}
              onClose={closeDocument}
              onCloseOthers={(id) => {
                const dirty = documents.filter((document) => document.id !== id && document.dirty);
                if (dirty.length && !window.confirm(`有 ${dirty.length} 个文件包含未保存的更改，确定关闭其他文件吗？`)) return false;
                for (const document of documents) {
                  if (document.id !== id) revokeBlobUrl(document.imageUrl);
                }
                setDocuments((items) => items.filter((document) => document.id === id));
                return true;
              }}
              onCloseAll={() => {
                const dirty = documents.filter((document) => document.dirty);
                if (dirty.length && !window.confirm(`有 ${dirty.length} 个文件包含未保存的更改，确定全部关闭吗？`)) return false;
                for (const document of documents) revokeBlobUrl(document.imageUrl);
                setDocuments([]);
                return true;
              }}
              onChange={changeDocument}
              onSave={(id) => void saveDocument(id)}
              onRefresh={(id) => void refreshDocument(id)}
              onReloadExternal={reloadExternalDocument}
              onKeepLocal={keepLocalDocument}
              onPreviewLoaded={() => undefined}
              onPreviewError={() => undefined}
              onRestorePreview={() => undefined}
              onMarkdownViewChange={(id, mode: MarkdownViewMode) => setDocuments((items) => items.map((document) => document.id === id ? { ...document, markdownView: mode } : document))}
            />
          </Suspense>
  );

  const renderSidebar = () => (
    <aside className="project-aow-right" hidden={!rightSidebarVisible}>
      <nav aria-label="AOW side views">
        <button className={rightView === 'terminals' ? 'active' : ''} title="Terminal" aria-label="Terminal 面板" onClick={() => setRightView('terminals')}><SquareTerminal /></button>
        <button className={rightView === 'sessions' ? 'active' : ''} title="Conversation" aria-label="Conversation" onClick={() => setRightView('sessions')}><MessageSquare /></button>
        <button className={rightView === 'automations' ? 'active' : ''} title="Automation" aria-label="Automation" onClick={() => setRightView('automations')}><CalendarClock /></button>
        <span className="project-aow-view-separator" aria-hidden="true" />
        <button className={rightView === 'files' && (!project.builtin || rightSidebarVisible) ? 'active' : ''} title="Explorer" aria-label="Explorer" onClick={() => setRightView('files')}><FolderOpen /></button>
        <button className={rightView === 'git' ? 'active' : ''} title="Source Control" aria-label="Source Control" onClick={() => setRightView('git')}><GitBranch /></button>
        <button className={rightView === 'pullRequests' ? 'active' : ''} title="Pull Requests" aria-label="Pull Requests" onClick={() => setRightView('pullRequests')}><GitPullRequest /></button>
        <button className="project-aow-right-hide" title="隐藏右侧栏" aria-label="隐藏右侧栏" onClick={onHideRightSidebar}><PanelRightClose /></button>
      </nav>
      <div className="project-aow-right-content" hidden={rightView !== 'files'}>
        <AowPanelStack className="project-aow-explorer-stack">
          <Explorer
            key={`files:${worktree.path}`} root={explorerRoot}
            title="Project Explorer" titleIcon={<Files />} aowHeader collapsed={filesExplorerCollapsed} onCollapsedChange={setFilesExplorerCollapsed}
            activePath={sidebarDocument?.explorerSource === 'project' ? sidebarDocument.path : undefined}
            lockedRoot titleCase renameDirectories onRootChange={() => undefined} onOpenFile={(entry) => void openExplorerFile(entry, 'project')}
            onRenameFile={(path, name) => renameExplorerEntry('project', path, name)}
            onCreateFile={(directory, name) => createExplorerEntry('project', directory, name, 'file')}
            onCreateDirectory={(directory, name) => createExplorerEntry('project', directory, name, 'directory')}
            onDeleteEntry={(path, directory) => void deleteExplorerEntry('project', path, directory)} refreshRequest={explorerRefresh}
          />
          <Explorer
            key={`notes:${project.id}:${project.notes_path}`} root={project.notes_path}
            title="Notes Explorer" titleIcon={<NotebookPen />} aowHeader collapsed={notesExplorerCollapsed} onCollapsedChange={setNotesExplorerCollapsed}
            activePath={activeNotePath} lockedRoot titleCase renameDirectories
            onRootChange={() => undefined} onOpenFile={(entry) => void openExplorerFile(entry, 'notes')}
            onRenameFile={(path, name) => renameExplorerEntry('notes', path, name)}
            onCreateFile={(directory, name) => createExplorerEntry('notes', directory, name, 'file')}
            onCreateDirectory={(directory, name) => createExplorerEntry('notes', directory, name, 'directory')}
            onDeleteEntry={(path, directory) => void deleteExplorerEntry('notes', path, directory)} refreshRequest={notesRefresh}
          />
        </AowPanelStack>
      </div>
      <div className="project-aow-right-content" hidden={rightView !== 'git'}>
        <SourceControl key={`git:${worktree.path}`} root={gitRoot} fixedRepository={repository} titleCase aowHeader refreshGeneration={sourceControlRefresh} visible={active && rightSidebarVisible && rightView === 'git'} activeDocumentId={sidebarDocument?.id} onOpenDiff={openDiff} onOpenCommitDiff={openCommitDiff} />
      </div>
      <div className="project-aow-right-content" hidden={rightView !== 'pullRequests'}>
        <PullRequestsPanel key={'pull-requests:' + worktree.path} repository={worktree.path} visible={active && rightSidebarVisible && rightView === 'pullRequests'} activeNumber={activePullRequest?.number} activeProvider={activePullRequest?.provider} activeRemote={activePullRequest?.remote} onOpen={openPullRequest} />
      </div>
      <div className="project-aow-right-content" hidden={rightView !== 'sessions'}>
        <AgentSessions key={`sessions:${worktree.path}`} worktreePath={sessionRoot} agents={agents} activeSessionId={activeSessionId} visible={active && rightSidebarVisible && rightView === 'sessions'} onOpen={openSession} onResume={(session, agent) => void createTerminal(agent, session, sessionRoot)} />
      </div>
      <div className="project-aow-right-content" hidden={rightView !== 'terminals'}>
        <TerminalPanel tabs={panelTerminals} activeId={activeTerminalId}
          detectedAgents={showAllTerminals ? { ...projectTerminals.agents, ...terminals.detectedAgents } : terminals.detectedAgents}
          titles={showAllTerminals ? { ...projectTerminals.titles, ...terminals.terminalTitles } : terminals.terminalTitles}
          worktrees={canShowAllTerminals ? project.worktrees : undefined} showAll={canShowAllTerminals ? terminalScopes : undefined}
          onShowAllChange={(group, value) => {
            const next = { ...terminalScopes, [group]: value };
            setTerminalScopes(next); persist(terminalScopeKey, next);
          }}
          sortBy={terminalSorts} onSortChange={(group, value) => {
            const next = { ...terminalSorts, [group]: value };
            setTerminalSorts(next); persist(terminalSortKey, next);
          }}
          loading={showAllTerminals && projectTerminals.loading} error={showAllTerminals ? projectTerminals.error : undefined}
          openedIds={new Set([...openedTerminals.map(tab => tab.id), ...hostedTabs.map(tab => tab.targetId)])}
          onReload={() => { void terminals.reload(); if (showAllTerminals) projectTerminals.reload(); }}
          onRebuild={async id => {
            const tab = await terminalApi.rebuild(id);
            terminals.replace(tab);
            projectTerminals.reload();
          }}
          onTerminate={async id => {
            if (terminals.tabs.some(tab => tab.id === id)) await terminals.close(id);
            else {
              try { await terminalApi.close(id); projectTerminals.remove(id); }
              catch (reason) { setOperationError(message(reason)); }
            }
          }}
          onOpen={openTerminal} onOpenFloating={tab => openTerminal(tab, true)} />
      </div>
      <div className="project-aow-right-content" hidden={rightView !== 'automations'}>
        <AutomationPanel project={project} agents={agents} activeTaskId={activeCenterId?.startsWith('automation:') ? activeCenterId.slice('automation:'.length) : undefined} refreshKey={automationRefreshKey} onOpenTask={openAutomation} onTaskChanged={taskChanged} onTaskDeleted={taskDeleted} />
      </div>
    </aside>
  );
  return <FloatingOpenMenu><section className="project-aow-surface" hidden={!active || project.builtin}>
    <main className="project-aow-center">
      <WorkspaceTabs workspaceKey={`project:${worktree.path}`} revealRequest={tabRevealRequest} tabs={centerTabs} activeId={activeCenterId} visible={active && !project.builtin} agents={agents}
        onActivate={activateCenter} onCloseTab={closeTab} onCloseTabs={(targets, selected) => closeTabsInGroup(selected, targets.map(tab => tab.id))}
        onCanRename={canRenameTab} onRename={renameTab} onError={setOperationError}
        destination={tab => hostedTab(tab.id) ? 'restore' : 'floating'} onOpenElsewhere={tab => {
          const hosted = hostedTab(tab.id);
          if (hosted) {
            floating.removeHosted(hosted.workspace, hosted.id);
            floating.sources.current.get(hosted.workspace)?.reveal(hosted.id);
          } else floating.add(worktree.path, tab.id);
        }}
        onOpenFile={() => { setOpenFileFloating(openingInFloatingWorkspace()); setShowOpenFile(true); }}
        onCreateNote={kind => void createTemporaryNote(kind)} onCreateTerminal={agent => void createTerminal(agent)}
        leadingControls={onShowLeftSidebar ? <LeftSidebarToggle onClick={onShowLeftSidebar} /> : null}
        controls={!rightSidebarVisible ? <button className="project-aow-right-show" title="显示右侧栏" aria-label="显示右侧栏" onClick={onShowRightSidebar}><PanelRightOpen /></button> : null}
      />
      {operationError || terminals.error ? <div className="project-aow-inline-error">{operationError || terminals.error}<button onClick={() => setOperationError('')}><X /></button></div> : null}
      <div className="project-aow-center-content">
        <div ref={setHostPortal} />
        {openedTerminals.map((tab) => <Fragment key={tab.id}>{projectContent(`terminal:${tab.id}`, <div className="project-aow-terminal-host" hidden={!tabVisible(`terminal:${tab.id}`)}>
          <TerminalWorkspace
            visible={tabLive(`terminal:${tab.id}`)}
            tab={tab}
            loading={terminals.loading}
            detectedAgents={terminals.detectedAgents}
            terminalTitles={terminals.terminalTitles}
            agentProcesses={terminals.agentProcesses}
            onTabChange={terminals.replace}
            onTabClosed={terminals.remove}
            onPaneStatus={terminals.updatePaneStatus}
            onReload={() => { void terminals.reload(); }}
          />
        </div>)}</Fragment>)}
        <div className="project-aow-document-host" hidden={!activeIsDocument || isFloating(activeCenterId ?? '')}>
          {documents.length > 0 ? renderEditor(isFloating(activeCenterId ?? '') ? undefined : activeDocumentId) : null}
        </div>
        {floatingId?.startsWith('document:') && floating.portal ? createPortal(<div className="floating-workspace-host">{renderEditor(floatingId.slice(9))}</div>, floating.portal, worktree.path + ':editor') : null}
        {sessionPreviews.map((preview) => <Fragment key={preview.session.id}>{projectContent(`session:${preview.session.id}`, <div className="project-aow-session-host" hidden={!tabVisible(`session:${preview.session.id}`)}>
          <SessionSnapshotView
            session={preview.session}
            actions={preview.snapshot && <SessionShareButton session={preview.session} workspacePath={preview.workspacePath} />}
            snapshot={preview.snapshot}
            loading={preview.loading}
            error={preview.error}
            onRefresh={() => preview.automationRun
              ? void resolveAutomationSession(preview.session, preview.automationRun)
              : void loadSessionSnapshot(preview.session, preview.workspacePath)}
          />
        </div>)}</Fragment>)}
        {openPullRequests.map((pr) => <Fragment key={prTabId(pr.repository ?? worktree.path, pr.number, pr)}>{projectContent(prTabId(pr.repository ?? worktree.path, pr.number, pr), <div className="project-aow-pull-request-host" hidden={!tabVisible(prTabId(pr.repository ?? worktree.path, pr.number, pr))}>
          <PullRequestDetailView
            repository={pr.repository ?? worktree.path}
            number={pr.number}
            provider={pr.provider}
            remote={pr.remote}
            visible={tabLive(prTabId(pr.repository ?? worktree.path, pr.number, pr))}
          />
        </div>)}</Fragment>)}
        {openAutomationTaskIds.map((taskId) => <Fragment key={taskId}>{projectContent(`automation:${taskId}`, <div className="project-aow-automation-host" hidden={!tabVisible(`automation:${taskId}`)}>
          <AutomationDetail
            taskId={taskId}
            refreshKey={automationRefreshKey}
            initialLocation={automationLocations[taskId]}
            onLocationChange={location => setAutomationLocations(items => items[taskId]?.view === location.view && items[taskId]?.runId === location.runId ? items : { ...items, [taskId]: location })}
            visible={tabLive(`automation:${taskId}`)}
            project={project}
            agents={agents}
            onTaskChanged={taskChanged}
            onTaskDeleted={taskDeleted}
            onOpenSession={(run) => openAutomationSession(taskId, run)}
          />
        </div>)}</Fragment>)}
        {browserOpened && floating.portal ? createPortal(<div className="floating-workspace-host" hidden={floatingId !== 'system-files' || !browserTab}><SystemFileBrowser navigationRequest={browserRequest} onPathChange={setBrowserPath} open={floating.visible && floatingId === 'system-files' && browserTab} onHide={floating.minimize} onOpenFile={entry => void openExplorerFile(entry, 'external', true)} /></div>, floating.portal, 'system-file-browser') : null}
        {activeCenterId && isFloating(activeCenterId) && !project.builtin ? <div className="project-aow-empty project-aow-tab-away">
          <span>此 Tab 正在浮动工作区显示。</span>
          <button type="button" onClick={() => floating.remove(worktree.path, activeCenterId)}><CornerDownLeft aria-hidden="true" />移回当前工作区</button>
        </div> : null}
        {activeCenterId && hostedLocation(activeCenterId) ? <div className="project-aow-empty project-aow-tab-away">
          <span>此 Tab 正在主仓库工作区显示。</span>
          <button type="button" onClick={() => floating.removeHosted(worktree.path, activeCenterId)}><CornerDownLeft aria-hidden="true" />移回当前工作区</button>
        </div> : null}
        {!activeCenterId ? <div className="project-aow-welcome"><ProjectIcon project={project} size={32} /><h2>{project.name}</h2><p>{worktree.path}</p><span>点击 + 新建 Terminal 或启动本地 Agent；从右侧打开文件和 Git Diff。</span></div> : null}
      </div>
    </main>
    <div className="project-aow-right-resizer" role="separator" aria-label="调整右侧栏宽度" aria-orientation="vertical" onPointerDown={onStartRightResize} />
    {project.builtin && floating.sidebar ? createPortal(renderSidebar(), floating.sidebar) : renderSidebar()}
    {showOpenFile ? <OpenFileDialog worktreePath={worktree.path} onClose={() => setShowOpenFile(false)} onOpen={path => openFileFloating ? withFloatingOpen(() => openExternalFile(path)) : openExternalFile(path)} /> : null}
    {(operationError || terminals.error) && floating.portal && (project.builtin || floatingId) ? createPortal(
      <div className="floating-workspace-error" role="alert">{operationError || terminals.error}<button aria-label="关闭错误提示" onClick={() => setOperationError('')}><X /></button></div>, floating.portal) : null}
  </section></FloatingOpenMenu>;
});

function ProjectAowContents({ initialEntry }: { initialEntry?: ResolvedTab }) {
  const floating = useFloatingWorkspace();
  const { confirm, confirmationDialog } = useConfirmation();
  const unreadCounts = useWorktreeUnreadCounts();
  const syncTabLocation = useAowTabLocation();
  const reportTabLocationError = useAowTabLocationError();
  const [floatingFocused, setFloatingFocused] = useState(floating.visible);
  const [globalProject, setGlobalProject] = useState<AowProject>();
  const [projects, setProjects] = useState<AowProject[]>([]);
  const operations = useOperations();
  const [logPanel, setLogPanel] = useState<{ operationId?: string }>();
  const removals = useWorktreeRemovals();
  const [cleanupProjectId, setCleanupProjectId] = useState<string>();
  const busyWorktrees = useMemo(() => new Set(removals.jobs.flatMap(job => job.items.filter(removalActive).map(item => item.path))), [removals.jobs]);
  const handledRemovals = useRef(new Set<string>());
  const applyingRemovals = useRef(false);
  const externalTabWorkspaces = useMemo(() => new Set([
    ...floating.tabs.map(tab => tab.workspace),
    ...floating.hostedTabs.flatMap(tab => [tab.workspace, tab.host]),
  ]), [floating.tabs, floating.hostedTabs]);
  const { isUnallocated, reportResources } = useWorktreeResources(projects, externalTabWorkspaces);
  const [agents, setAgents] = useState<AowAgent[]>([]);
  const [collapsedProjects, setCollapsedProjects] = useState<Record<string, boolean>>({});
  const [activeWorktreePath, setActiveWorktreePath] = useState(() => initialEntry?.workspacePath ?? appLocalStorage.getItem('aow-active') ?? '');
  const [visitedWorktrees, setVisitedWorktrees] = useState<Set<string>>(() => new Set());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [showRegisterProject, setShowRegisterProject] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [nodeAddresses, setNodeAddresses] = useState<string[]>();
  const [leftSidebarWidth, setLeftSidebarWidth] = useState(() => storedWidth(leftSidebarWidthStorageKey, defaultLeftSidebarWidth, minLeftSidebarWidth, maxLeftSidebarWidth));
  const [leftSidebarVisible, setLeftSidebarVisible] = useState(() => appLocalStorage.getItem(leftSidebarVisibleStorageKey) !== 'false');
  const [rightSidebarWidth, setRightSidebarWidth] = useState(() => storedWidth(rightSidebarWidthStorageKey, defaultRightSidebarWidth, minRightSidebarWidth, maxRightSidebarWidth));
  const [rightSidebarVisible, setRightSidebarVisible] = useState(() => appLocalStorage.getItem(rightSidebarVisibleStorageKey) !== 'false');
  const [pinnedWorktrees, setPinnedWorktrees, pinnedState] = usePinnedWorktrees();
  const [draggingPinnedPath, setDraggingPinnedPath] = useState<string>();
  const [pinnedDropTarget, setPinnedDropTarget] = useState<PinnedDropTarget>();
  const [worktreeContextMenu, setWorktreeContextMenu] = useState<WorktreeContextMenuState>();
  const [savingWorktreeAppearance, setSavingWorktreeAppearance] = useState(false);
  const [removeWorktreeState, setRemoveWorktreeState] = useState<RemoveWorktreeState>();
  const [projectMenu, setProjectMenu] = useState<ProjectMenuState>();
  const [createWorktreeProject, setCreateWorktreeProject] = useState<AowProject>();
  const [bindNotesProject, setBindNotesProject] = useState<AowProject>();
  const [notesRefreshByProject, setNotesRefreshByProject] = useState<Record<string, ExplorerRefresh>>({});
  const layoutRef = useRef<HTMLDivElement>(null);
  const draggingPinnedPathRef = useRef<string | undefined>(undefined);
  const suppressPinnedClickRef = useRef(false);

  const projectNotesPaths = useRef(new Map<string, string>());
  const loadProjects = useCallback(async (notesMoved = false) => {
    setLoading(true);
    setError('');
    try {
      const all = await aowApi.projects();
      if (notesMoved) floating.setNotesMoves(all.flatMap(project => {
        const from = projectNotesPaths.current.get(project.id);
        return from && from !== project.notes_path ? [{ from, to: project.notes_path }] : [];
      }));
      projectNotesPaths.current = new Map(all.map(project => [project.id, project.notes_path]));
      const builtin = all.find(project => project.builtin);
      setGlobalProject(builtin);
      floating.retainWorkspaces(all.flatMap(project => project.worktrees.map(worktree => worktree.path)));
      floating.setGlobalRoot(builtin?.worktrees.find(worktree => worktree.is_main)?.path ?? builtin?.worktrees[0]?.path ?? '');
      const next = all.filter(project => !project.builtin);
      setProjects(next);
      const paths = next.flatMap((project) => project.worktrees.map((worktree) => worktree.path));
      setActiveWorktreePath((current) => paths.includes(current) ? current : paths[0] ?? '');
    } catch (reason) {
      setError(message(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  const loadAgents = useCallback(async () => {
    try { setAgents(await agentsApi.agents(true)); } catch (reason) { setError(message(reason)); }
  }, []);

  useEffect(() => { void Promise.all([loadProjects(), loadAgents()]); }, [loadAgents, loadProjects]);
  useEffect(() => {
    let active = true;
    void aowApi.settings().then(settings => {
      // A later Settings load/save takes precedence over the initial request.
      if (active) setNodeAddresses(current => current ?? settings.node_addresses ?? []);
    }).catch(reason => {
      if (active) { setNodeAddresses(current => current ?? []); setError(message(reason)); }
    });
    return () => { active = false; };
  }, []);
  useEffect(() => { appLocalStorage.setItem(leftSidebarWidthStorageKey, String(leftSidebarWidth)); }, [leftSidebarWidth]);
  useEffect(() => { appLocalStorage.setItem(leftSidebarVisibleStorageKey, String(leftSidebarVisible)); }, [leftSidebarVisible]);
  useEffect(() => { appLocalStorage.setItem(rightSidebarWidthStorageKey, String(rightSidebarWidth)); }, [rightSidebarWidth]);
  useEffect(() => { appLocalStorage.setItem(rightSidebarVisibleStorageKey, String(rightSidebarVisible)); }, [rightSidebarVisible]);
  useEffect(() => {
    if (activeWorktreePath) appLocalStorage.setItem('aow-active', activeWorktreePath);
    else appLocalStorage.removeItem('aow-active');
    if (!activeWorktreePath) return;
    setVisitedWorktrees((current) => current.has(activeWorktreePath) ? current : new Set(current).add(activeWorktreePath));
  }, [activeWorktreePath]);
  useEffect(() => {
    setVisitedWorktrees(current => {
      const paths = floating.hostedTabs.flatMap(tab => [tab.workspace, tab.host]);
      return paths.some(path => !current.has(path)) ? new Set([...current, ...paths]) : current;
    });
  }, [floating.hostedTabs]);

  const worktreeEntries = useMemo(() => projects.flatMap((project) => project.worktrees.map((worktree) => ({ project, worktree }))), [projects]);
  const activeEntry = worktreeEntries.find(({ worktree }) => worktree.path === activeWorktreePath);
  const contextMenuEntry = worktreeContextMenu && worktreeEntries.find(({ worktree }) => worktree.path === worktreeContextMenu.worktree.path);
  const locationInFloating = floating.visible && !!floating.active && (floatingFocused || !activeEntry);
  const locationSource = (path: string) => locationInFloating
    ? floating.active?.workspace === path ? 'floating' as const : undefined
    : activeWorktreePath === path ? 'center' as const : undefined;
  useEffect(() => { setFloatingFocused(false); }, [activeWorktreePath]);
  useEffect(() => { if (floating.showRequest) setFloatingFocused(true); }, [floating.showRequest]);
  useEffect(() => {
    if (!initialEntry || loading) return;
    const owner = [...projects, ...(globalProject ? [globalProject] : [])]
      .find(project => project.worktrees.some(worktree => worktree.path === initialEntry.workspacePath));
    if (!owner) { reportTabLocationError('目标 Tab 所属工作区不存在或已被移除。'); return; }
    const hosted = floating.hostedTabs.find(tab => tab.workspace === initialEntry.workspacePath && tab.id === tabCenterId(initialEntry.target));
    if (!owner.builtin) setActiveWorktreePath(hosted?.host ?? initialEntry.workspacePath);
    setCollapsedProjects(current => ({ ...current, [owner.id]: false }));
    const inFloating = owner.builtin || floating.contains(initialEntry.workspacePath, tabCenterId(initialEntry.target));
    setFloatingFocused(!!inFloating);
    if (inFloating) floating.add(initialEntry.workspacePath, tabCenterId(initialEntry.target));
  }, [initialEntry, loading, projects, globalProject, floating.add, floating.contains, floating.hostedTabs, reportTabLocationError]);
  useEffect(() => {
    if (!loading && !activeEntry && !locationInFloating) syncTabLocation();
  }, [loading, activeEntry, locationInFloating, syncTabLocation]);
  const pinnedEntries = useMemo(() => {
    const entries = new Map(worktreeEntries.map((entry) => [entry.worktree.path, entry]));
    return [...pinnedWorktrees].flatMap((path) => {
      const entry = entries.get(path);
      return entry ? [entry] : [];
    });
  }, [pinnedWorktrees, worktreeEntries]);
  // Resource allocation and display exceptions are separate rules.
  const compactWorktrees = useMemo(() => new Set(worktreeEntries
    .filter(({ worktree }) => !worktree.is_main && !pinnedWorktrees.has(worktree.path)
      && isUnallocated(worktree.path))
    .map(({ worktree }) => worktree.path)), [worktreeEntries, pinnedWorktrees, isUnallocated]);

  const openWorktreeContextMenu = (event: React.MouseEvent, entry: WorktreeEntry) => {
    event.preventDefault();
    event.stopPropagation();
    setProjectMenu(undefined);
    setWorktreeContextMenu({ ...entry, x: event.clientX, y: event.clientY });
  };

  const openProjectMenu = (event: React.MouseEvent<HTMLButtonElement>, project: AowProject) => {
    event.stopPropagation();
    const bounds = event.currentTarget.getBoundingClientRect();
    setWorktreeContextMenu(undefined);
    setProjectMenu((current) => current?.project.id === project.id
      ? undefined
      : { project, x: bounds.right - 220, y: bounds.bottom + 2 });
  };

  const togglePinnedWorktree = (path: string) => {
    setPinnedWorktrees((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const startPinnedDrag = (event: ReactDragEvent<HTMLButtonElement>, path: string) => {
    setWorktreeContextMenu(undefined);
    draggingPinnedPathRef.current = path;
    suppressPinnedClickRef.current = true;
    setDraggingPinnedPath(path);
    setPinnedDropTarget(undefined);
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('application/x-aow-pinned-worktree', path);
    event.dataTransfer.setData('text/plain', path);
  };

  const updatePinnedDropTarget = (event: ReactDragEvent<HTMLButtonElement>, path: string) => {
    const sourcePath = draggingPinnedPathRef.current
      || draggingPinnedPath
      || event.dataTransfer.getData('application/x-aow-pinned-worktree');
    if (!sourcePath || sourcePath === path) {
      setPinnedDropTarget(undefined);
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    const bounds = event.currentTarget.getBoundingClientRect();
    const position = event.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after';
    setPinnedDropTarget((current) => current?.path === path && current.position === position
      ? current
      : { path, position });
  };

  const dropPinnedWorktree = (event: ReactDragEvent<HTMLButtonElement>, targetPath: string) => {
    event.preventDefault();
    const sourcePath = draggingPinnedPathRef.current
      || draggingPinnedPath
      || event.dataTransfer.getData('application/x-aow-pinned-worktree')
      || event.dataTransfer.getData('text/plain');
    const bounds = event.currentTarget.getBoundingClientRect();
    const position = event.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after';
    if (sourcePath && sourcePath !== targetPath) {
      setPinnedWorktrees((current) => {
        if (!current.has(sourcePath) || !current.has(targetPath)) return current;
        const ordered = [...current].filter((path) => path !== sourcePath);
        const targetIndex = ordered.indexOf(targetPath);
        ordered.splice(targetIndex + (position === 'after' ? 1 : 0), 0, sourcePath);
        return new Set(ordered);
      });
    }
    setDraggingPinnedPath(undefined);
    setPinnedDropTarget(undefined);
  };

  const finishPinnedDrag = () => {
    draggingPinnedPathRef.current = undefined;
    setDraggingPinnedPath(undefined);
    setPinnedDropTarget(undefined);
    window.setTimeout(() => { suppressPinnedClickRef.current = false; }, 0);
  };

  const sidebarDimensions = useRef({ leftSidebarWidth, leftSidebarVisible, rightSidebarWidth, rightSidebarVisible });
  sidebarDimensions.current = { leftSidebarWidth, leftSidebarVisible, rightSidebarWidth, rightSidebarVisible };
  const startSidebarResize = useCallback((side: 'left' | 'right', event: ReactPointerEvent<HTMLDivElement>) => {
    const { leftSidebarWidth, leftSidebarVisible, rightSidebarWidth, rightSidebarVisible } = sidebarDimensions.current;
    event.preventDefault();
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startWidth = side === 'left' ? leftSidebarWidth : rightSidebarWidth;
    const layoutWidth = layoutRef.current?.getBoundingClientRect().width ?? window.innerWidth;
    handle.setPointerCapture(pointerId);
    document.body.classList.add('resizing-project-aow');

    const move = (moveEvent: PointerEvent) => {
      const delta = moveEvent.clientX - startX;
      if (side === 'left') {
        const available = layoutWidth - (rightSidebarVisible ? rightSidebarWidth : 0) - minCenterWidth;
        const max = Math.max(minLeftSidebarWidth, Math.min(maxLeftSidebarWidth, available));
        setLeftSidebarWidth(Math.min(max, Math.max(minLeftSidebarWidth, startWidth + delta)));
      } else {
        const available = layoutWidth - (leftSidebarVisible ? leftSidebarWidth : 0) - minCenterWidth;
        const max = Math.max(minRightSidebarWidth, Math.min(maxRightSidebarWidth, available));
        setRightSidebarWidth(Math.min(max, Math.max(minRightSidebarWidth, startWidth - delta)));
      }
    };
    const stop = () => {
      if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
      document.body.classList.remove('resizing-project-aow');
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', stop);
      window.removeEventListener('pointercancel', stop);
    };
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', stop);
    window.addEventListener('pointercancel', stop);
  }, []);
  const hideLeftSidebar = useCallback(() => setLeftSidebarVisible(false), []);
  const showLeftSidebar = useCallback(() => setLeftSidebarVisible(true), []);
  const hideRightSidebar = useCallback(() => setRightSidebarVisible(false), []);
  const showRightSidebar = useCallback(() => setRightSidebarVisible(true), []);
  const startRightResize = useCallback((event: ReactPointerEvent<HTMLDivElement>) => startSidebarResize('right', event), [startSidebarResize]);

  const projectRegistered = async (project: AowProject) => {
    await loadProjects();
    setCollapsedProjects(current => ({ ...current, [project.id]: false }));
    setActiveWorktreePath(project.worktrees[0]?.path ?? project.registered_path);
    setShowRegisterProject(false);
  };

  const removeProject = async (project: AowProject) => {
    if (!await confirm({
      title: '移除 Project？',
      description: '移除后将取消该项目的注册，不会删除磁盘上的仓库或 Worktree。',
      items: [{ id: project.id, label: project.name, detail: project.registered_path, icon: <ProjectIcon project={project} /> }],
      confirmLabel: '移除 Project',
      danger: true,
    })) return;
    try { await aowApi.removeProject(project.id); await loadProjects(); } catch (reason) { setError(message(reason)); }
  };

  const inspectWorktreeRemoval = async (entry: WorktreeEntry) => {
    if (busyWorktrees.has(entry.worktree.path)) { setCleanupProjectId(entry.project.id); return; }
    setError('');
    try {
      const preview = await aowApi.inspectWorktreeRemoval(entry.project.id, entry.worktree.path);
      setRemoveWorktreeState({ ...entry, preview });
    } catch (reason) {
      setError(message(reason));
    }
  };

  const changeWorktreeAppearance = async (entry: WorktreeEntry, appearance: { color: WorktreeColor } | { icon: WorktreeIconId }) => {
    if (savingWorktreeAppearance) return;
    const previous = 'color' in appearance
      ? { color: entry.worktree.color ?? 'default' }
      : { icon: entry.worktree.icon ?? 'default' };
    if ('color' in appearance ? appearance.color === previous.color : appearance.icon === previous.icon) return;
    setError('');
    setSavingWorktreeAppearance(true);
    setProjects((items) => updateWorktreeAppearance(items, entry.worktree.path, appearance));
    try {
      if ('color' in appearance) await aowApi.setWorktreeColor(entry.project.id, entry.worktree.path, appearance.color);
      else await aowApi.setWorktreeIcon(entry.project.id, entry.worktree.path, appearance.icon);
    } catch (reason) {
      setProjects((items) => updateWorktreeAppearance(items, entry.worktree.path, previous));
      setError(message(reason));
    } finally {
      setSavingWorktreeAppearance(false);
    }
  };

  const worktreeRemoved = (project: AowProject, removedPath: string) => {
    for (const tab of floating.tabs) if (tab.workspace === removedPath) floating.remove(tab.workspace, tab.id);
    for (const tab of floating.hostedTabs) if (tab.workspace === removedPath || tab.host === removedPath) floating.removeHosted(tab.workspace, tab.id);
    setProjects((items) => items.map((item) => item.id === project.id ? project : item));
    setPinnedWorktrees((current) => {
      if (!current.has(removedPath)) return current;
      const next = new Set(current);
      next.delete(removedPath);
      return next;
    });
    setVisitedWorktrees((current) => {
      if (!current.has(removedPath)) return current;
      const next = new Set(current);
      next.delete(removedPath);
      return next;
    });
    setActiveWorktreePath((current) => {
      if (current !== removedPath) return current;
      return project.worktrees.find((worktree) => worktree.is_main)?.path
        ?? project.worktrees[0]?.path
        ?? '';
    });
    appLocalStorage.removeItem(`aow-workspace-tabs:${removedPath}`);
  };

  useEffect(() => {
    const completed = removals.jobs.flatMap(job => job.items.filter(item => item.status === 'succeeded')
      .map(item => ({ key: `${job.id}:${item.path}`, projectId: job.project_id, path: item.path })))
      .filter(item => !handledRemovals.current.has(item.key));
    if (!completed.length || applyingRemovals.current) return;
    applyingRemovals.current = true;
    void aowApi.projects().then(all => {
      for (const item of completed) {
        const project = all.find(project => project.id === item.projectId);
        // History must never remove a newly created worktree at an old path.
        if (project && !project.worktrees.some(worktree => worktree.path === item.path)) worktreeRemoved(project, item.path);
        handledRemovals.current.add(item.key);
      }
    }).catch(reason => setError(`Worktree 已删除，但刷新项目失败：${message(reason)}`))
      .finally(() => { applyingRemovals.current = false; });
  }, [removals.jobs]);

  useEffect(() => {
    for (const tab of floating.tabs) if (busyWorktrees.has(tab.workspace)) floating.remove(tab.workspace, tab.id);
    for (const tab of floating.hostedTabs) if (busyWorktrees.has(tab.workspace) || busyWorktrees.has(tab.host)) floating.removeHosted(tab.workspace, tab.id);
  }, [busyWorktrees, floating.tabs, floating.hostedTabs]);

  const removalSubmitted = (job: WorktreeRemovalJob) => {
    removals.submitted(job);
    setRemoveWorktreeState(undefined);
    setCleanupProjectId(job.project_id);
  };

  const worktreeCreated = (project: AowProject, worktree: AowWorktree) => {
    setProjects((items) => items.map((item) => item.id === project.id ? project : item));
    setCollapsedProjects(current => ({ ...current, [project.id]: false }));
    setActiveWorktreePath(worktree.path);
    setCreateWorktreeProject(undefined);
  };

  const notesBound = (project: AowProject) => {
    projectNotesPaths.current.set(project.id, project.notes_path);
    setProjects((items) => items.map((item) => item.id === project.id ? project : item));
    setBindNotesProject(undefined);
  };

  const notesChanged = useCallback((projectId: string, directory: string) => {
    setNotesRefreshByProject((current) => ({
      ...current,
      [projectId]: {
        generation: (current[projectId]?.generation ?? 0) + 1,
        directory,
      },
    }));
  }, []);

  useEffect(() => {
    document.title = activeEntry ? `${activeEntry.project.name} · ${activeEntry.worktree.branch || 'detached'} · AOW` : 'Project AOW';
  }, [activeEntry]);

  return <div className="project-aow"
    onPointerDownCapture={event => setFloatingFocused(!!(event.target as HTMLElement).closest('[data-floating-workspace]'))}
    onFocusCapture={event => setFloatingFocused(!!(event.target as HTMLElement).closest('[data-floating-workspace]'))}>
    <div
      className={`project-aow-layout${leftSidebarVisible ? '' : ' left-sidebar-hidden'}${rightSidebarVisible ? '' : ' right-sidebar-hidden'}`}
      ref={layoutRef}
      style={{ '--project-left-width': `${leftSidebarWidth}px`, '--project-right-width': `${rightSidebarWidth}px` } as CSSProperties}
    >
      <aside id="aow-project-sidebar" className="project-aow-projects" aria-label="项目侧边栏" hidden={!leftSidebarVisible}>
        <header className="project-aow-brand"><AowNodeSwitcher addresses={nodeAddresses} /><LeftSidebarToggle expanded onClick={hideLeftSidebar} /></header>
        <AowPanelStack className="project-aow-project-list">
          <AowPanel title="Pinned" icon={<Pin />} empty={!pinnedEntries.length && !pinnedState.error}>
          {pinnedState.error ? <div className="project-aow-error" role="alert">{pinnedState.error}<button onClick={pinnedState.reload}>重试</button></div> : null}
          <section className="project-aow-pinned">
            {pinnedEntries.map((entry) => <button
              key={entry.worktree.path}
              className={[
                entry.worktree.path === activeWorktreePath ? 'active' : '',
                entry.worktree.path === draggingPinnedPath ? 'dragging' : '',
                pinnedDropTarget?.path === entry.worktree.path ? `drop-${pinnedDropTarget.position}` : '',
              ].filter(Boolean).join(' ')}
              disabled={busyWorktrees.has(entry.worktree.path)}
              draggable={!busyWorktrees.has(entry.worktree.path)}
              onClick={() => { if (!suppressPinnedClickRef.current) setActiveWorktreePath(entry.worktree.path); }}
              onContextMenu={(event) => openWorktreeContextMenu(event, entry)}
              onDragStart={(event) => startPinnedDrag(event, entry.worktree.path)}
              onDragOver={(event) => updatePinnedDropTarget(event, entry.worktree.path)}
              onDrop={(event) => dropPinnedWorktree(event, entry.worktree.path)}
              onDragEnd={finishPinnedDrag}
              title={entry.worktree.path}
            ><WorktreeIcon icon={entry.worktree.icon} style={{ color: worktreeColorValues[entry.worktree.color ?? 'default'] }} /><span><strong>{entry.worktree.detached ? 'detached' : entry.worktree.branch || basename(entry.worktree.path)}</strong><small>{entry.project.name} · {basename(entry.worktree.path)}</small></span>{entry.worktree.locked ? <i>locked</i> : null}<WorktreeUnreadBadge count={unreadCounts.get(entry.worktree.path)} /></button>)}
            {!pinnedEntries.length ? <div className="side-empty">暂无固定的 Worktree</div> : null}
          </section>
          </AowPanel>
          <AowPanel headerClassName="project-aow-project-list-title" title="Projects" empty={!projects.length && !loading && !error} icon={<FolderGit2 />} actions={<><AowIconButton title="刷新全部" aria-label="刷新全部" onClick={() => void loadProjects()}><RefreshCw /></AowIconButton><AowIconButton title="注册项目" aria-label="注册项目" onClick={() => setShowRegisterProject(true)}><Plus /></AowIconButton></>}>
          {error ? <div className="project-aow-error">{error}</div> : null}
          {loading ? <div className="project-aow-empty">正在加载…</div> : null}
          {projects.map((project) => {
            const orderedWorktrees = [
              ...project.worktrees.filter(worktree => !compactWorktrees.has(worktree.path)),
              ...project.worktrees.filter(worktree => compactWorktrees.has(worktree.path)),
            ];
            return <AowPanel key={project.id} className="project-aow-project" headerClassName="project-aow-project-row"
              title={project.name} tooltip={project.registered_path} icon={<ProjectIcon project={project} />} collapsed={collapsedProjects[project.id]}
              empty={!project.worktrees.length && !project.error} onCollapsedChange={collapsed => setCollapsedProjects(current => ({ ...current, [project.id]: collapsed }))}
              actions={<><AowIconButton title="刷新 Worktree" aria-label={`刷新 ${project.name} Worktree`} onClick={() => {
                void aowApi.refreshProject(project.id).then(updated => setProjects(items => items.map(item => item.id === project.id ? updated : item)))
                  .catch(reason => setError(message(reason)));
              }}><RefreshCw /></AowIconButton><AowIconButton title="Project 操作" aria-label={`${project.name} Project 操作`} aria-haspopup="menu"
                aria-expanded={projectMenu?.project.id === project.id} onClick={event => openProjectMenu(event, project)}><MoreHorizontal /></AowIconButton></>}>
              {project.error ? <div className="project-aow-project-error">{project.error}</div> : null}
              {orderedWorktrees.length ? <div className="project-aow-worktrees">{orderedWorktrees.map((worktree) => {
                const compact = compactWorktrees.has(worktree.path);
                const branch = worktree.detached ? 'detached' : worktree.branch || basename(worktree.path);
                return <button key={worktree.id} disabled={busyWorktrees.has(worktree.path)} className={[
                  worktree.path === activeWorktreePath ? 'active' : '', compact ? 'compact' : '',
                ].filter(Boolean).join(' ')} onClick={() => setActiveWorktreePath(worktree.path)} onContextMenu={(event) => openWorktreeContextMenu(event, { project, worktree })} title={worktree.path}>
                  {compact ? <><span className="project-aow-worktree-dot" aria-hidden="true" /><span className="project-aow-worktree-branch">{branch}</span></>
                    : <><WorktreeIcon icon={worktree.icon} style={{ color: worktreeColorValues[worktree.color ?? 'default'] }} /><span><strong>{branch}</strong><small>{basename(worktree.path)}{worktree.is_main ? ' · main worktree' : ''}</small></span>{worktree.locked ? <i>locked</i> : null}</>}
                  <WorktreeUnreadBadge count={unreadCounts.get(worktree.path)} />
                </button>;
              })}</div> : null}
            </AowPanel>;
          })}
          {!loading && !projects.length ? <div className="project-aow-empty project-aow-onboarding"><FolderGit2 /><strong>暂无项目</strong><span>注册本机 Git 仓库后，这里会自动列出它的 linked worktrees。</span><button onClick={() => setShowRegisterProject(true)}><Plus />注册项目</button></div> : null}
          </AowPanel>
        </AowPanelStack>
        <div className="project-aow-sidebar-footer">
          <button title="Settings" aria-label="Settings" onClick={() => setShowSettings(true)}><Settings /></button>
          <a href="https://yorkart.github.io/aow/" target="_blank" rel="noopener noreferrer" title="产品介绍（新窗口打开）" aria-label="产品介绍（新窗口打开）"><CircleHelp aria-hidden="true" /></a>
        </div>
      </aside>
      <div className="project-aow-left-resizer" role="separator" aria-label="调整项目栏宽度" aria-orientation="vertical" onPointerDown={(event) => startSidebarResize('left', event)} />
      <div className="project-aow-surfaces">
        {worktreeEntries.filter(({ worktree }) => !busyWorktrees.has(worktree.path)).filter(({ worktree }) => visitedWorktrees.has(worktree.path) || floating.tabs.some(tab => tab.workspace === worktree.path)
          || floating.hostedTabs.some(tab => tab.workspace === worktree.path || tab.host === worktree.path)).map(({ project, worktree }) => <WorkspaceSurface locationSource={locationSource(worktree.path)} initialEntry={initialEntry && (initialEntry.workspacePath === worktree.path
            || floating.hostedTabs.some(tab => tab.host === worktree.path && tab.workspace === initialEntry.workspacePath && tab.id === tabCenterId(initialEntry.target))) ? initialEntry : undefined} key={worktree.path} project={project} worktree={worktree} active={worktree.path === activeWorktreePath} agents={agents} onShowLeftSidebar={leftSidebarVisible ? undefined : showLeftSidebar} rightSidebarVisible={rightSidebarVisible} onHideRightSidebar={hideRightSidebar} onShowRightSidebar={showRightSidebar} onStartRightResize={startRightResize} notesRefresh={notesRefreshByProject[project.id] ?? initialExplorerRefresh} onNotesChanged={notesChanged} onResourcesChanged={reportResources} />)}
        {activeEntry && busyWorktrees.has(activeEntry.worktree.path) ? <div className="project-aow-no-context"><LoaderCircle className="spinning" /><p>该 Worktree 正在清理，可继续使用其他工作区。</p><button onClick={() => setCleanupProjectId(activeEntry.project.id)}>查看清理进度</button></div> : null}
        {!activeEntry && !leftSidebarVisible ? <nav className="project-aow-center-tabs project-aow-empty-toolbar"><LeftSidebarToggle onClick={showLeftSidebar} /></nav> : null}
        {!activeEntry ? <div className="project-aow-no-context"><FolderGit2 /><h1>Project AOW</h1><p>从左侧注册并选择一个 Project Worktree。</p></div> : null}
      </div>
    </div>
    <footer className="project-aow-status"><span>{activeEntry?.worktree.path ?? 'No active worktree'}</span>
      <OperationStatus operations={operations.operations} ready={!!operations.boot_id} error={operations.log_error || operations.error} logOpen={!!logPanel}
        onOpenOperation={operation => {
          if (operation.kind === 'worktree.remove' && operation.project_id && projects.some(project => project.id === operation.project_id)) {
            setCleanupProjectId(operation.project_id); void removals.refresh();
          } else setLogPanel({ operationId: operation.id });
        }}
        onOpenLogs={operationId => setLogPanel(current => operationId ? { operationId } : current && !current.operationId ? undefined : {})} />
      <span>{projects.length} projects · {worktreeEntries.length} worktrees</span>
    </footer>
    {logPanel && <OperationLogPanel key={logPanel.operationId ?? 'all'} initialOperationId={logPanel.operationId} operations={operations.operations}
      revision={operations.revision} bootId={operations.boot_id} logError={operations.log_error} onClose={() => setLogPanel(undefined)} />}
    {worktreeContextMenu && contextMenuEntry ? <WorktreeContextMenu state={{ ...worktreeContextMenu, ...contextMenuEntry }} savingAppearance={savingWorktreeAppearance} pinned={pinnedWorktrees.has(worktreeContextMenu.worktree.path)} onClose={() => setWorktreeContextMenu(undefined)} onTogglePin={() => togglePinnedWorktree(worktreeContextMenu.worktree.path)} onColorChange={(color) => void changeWorktreeAppearance(contextMenuEntry, { color })} onIconChange={(icon) => void changeWorktreeAppearance(contextMenuEntry, { icon })} onRemove={() => void inspectWorktreeRemoval(worktreeContextMenu)} /> : null}
    {removeWorktreeState ? <RemoveWorktreeDialog state={removeWorktreeState} onClose={() => setRemoveWorktreeState(undefined)} onSubmitted={removalSubmitted} /> : null}
    {projectMenu ? <ProjectMenu state={projectMenu} onClose={() => setProjectMenu(undefined)} onCreateWorktree={() => setCreateWorktreeProject(projectMenu.project)} onCleanup={() => { setCleanupProjectId(projectMenu.project.id); void removals.refresh(); }} onBindNotes={() => setBindNotesProject(projectMenu.project)} onRemove={() => void removeProject(projectMenu.project)} /> : null}
    {confirmationDialog}
    {cleanupProjectId && projects.find(project => project.id === cleanupProjectId) ? <WorktreeCleanupDialog key={cleanupProjectId} project={projects.find(project => project.id === cleanupProjectId)!} isUnallocated={isUnallocated} jobs={removals.jobs.filter(job => job.project_id === cleanupProjectId)} progressError={removals.error} onRefresh={() => void removals.refresh()} onSubmitted={removals.submitted} onClose={() => { removals.dismiss(cleanupProjectId); setCleanupProjectId(undefined); }} /> : null}
    {showRegisterProject ? <RegisterProjectDialog onClose={() => setShowRegisterProject(false)} onRegistered={projectRegistered} /> : null}
    {createWorktreeProject ? <CreateWorktreeDialog project={createWorktreeProject} onClose={() => setCreateWorktreeProject(undefined)} onCreated={worktreeCreated} /> : null}
    {bindNotesProject ? <BindNotesDialog project={bindNotesProject} onClose={() => setBindNotesProject(undefined)} onBound={notesBound} /> : null}
    {showSettings ? <SettingsDialog agents={agents} onClose={() => setShowSettings(false)} onNodesChange={setNodeAddresses} onReload={async notesMoved => { await Promise.all([loadAgents(), loadProjects(notesMoved)]); }} /> : null}
    <FloatingWorkspace agents={agents} />
    {globalProject && globalProject.worktrees[0] && (floating.open || floating.tabs.length > 0) ? <WorkspaceSurface locationSource={locationSource(floating.globalRoot)} initialEntry={globalProject.worktrees.some(worktree => worktree.path === initialEntry?.workspacePath) ? initialEntry : undefined} key={globalProject.id} project={globalProject} worktree={globalProject.worktrees.find(worktree => worktree.is_main) ?? globalProject.worktrees[0]} active={floating.visible} agents={agents} rightSidebarVisible={floating.sidebarOpen} onHideRightSidebar={() => floating.setSidebarOpen(false)} onShowRightSidebar={() => floating.setSidebarOpen(true)} onStartRightResize={() => undefined} notesRefresh={notesRefreshByProject[globalProject.id] ?? initialExplorerRefresh} onNotesChanged={notesChanged} /> : null}
  </div>;
}

export function ProjectAow({ initialEntry }: { initialEntry?: ResolvedTab } = {}) {
  return <EditorSettingsProvider><FloatingWorkspaceProvider><ProjectAowContents initialEntry={initialEntry} /></FloatingWorkspaceProvider></EditorSettingsProvider>;
}
