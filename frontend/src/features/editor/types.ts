import type { TextFile } from '../files/types';
export type PreviewKind = 'text' | 'markdown' | 'json' | 'image' | 'pdf' | 'docx' | 'diff' | 'unsupported';

export type MarkdownViewMode = 'preview' | 'editor';

export type DocumentSource = 'project' | 'notes' | 'external';

export interface OpenDocument {
  id: string;
  path: string;
  name: string;
  kind: PreviewKind;
  instanceId?: number;
  loadRequestId?: number;
  editRevision?: number;
  language?: string;
  mime?: string;
  content?: string;
  savedContent?: string;
  originalContent?: string;
  diffSource?: { repository: string } & (
    | { kind: 'working'; staged: boolean; untracked: boolean }
    | { kind: 'commit'; commit: string; originalPath?: string }
  );
  version?: string;
  dirty?: boolean;
  readOnly?: boolean;
  markdownView?: MarkdownViewMode;
  imageUrl?: string;
  previewReady?: boolean;
  previewHistory?: string[];
  error?: string;
  loading?: boolean;
  refreshing?: boolean;
  refreshError?: string;
  pendingExternal?: TextFile;
  saving?: boolean;
  explorerSource?: DocumentSource;
}

export interface EditorSettings {
  word_wrap: boolean;
}
