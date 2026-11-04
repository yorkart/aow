export type FileKind = 'directory' | 'file' | 'symlink' | 'other';

export interface FileEntry {
  name: string;
  path: string;
  kind: FileKind;
  size: number;
  modified_ms: number | null;
  readonly: boolean;
  hidden: boolean;
  mode: number;
  links: number;
  uid: number;
  gid: number;
  is_symlink: boolean;
  link_target: string | null;
}

export interface DirectoryListing {
  path: string;
  entries: FileEntry[];
}

export interface TextFile {
  path: string;
  content: string;
  size: number;
  version: string;
  language: string;
  mime: string;
}

export interface PinnedDirectories {
  paths: string[];
  revision: number;
}
