export interface RepositorySummary { path: string; name: string; branch: string }

export interface GitIgnoredPaths { repository: string | null; ignored: string[] }

export interface GitFileStatus {
  path: string;
  index_status: string;
  worktree_status: string;
  original_path: string | null;
}

export interface GitStatus {
  repository: string;
  branch: string;
  ahead: number;
  behind: number;
  files: GitFileStatus[];
}

export interface GitCommit {
  id: string;
  short_id: string;
  author: string;
  authored_at: string;
  subject: string;
  parents: string[];
  is_pushed: boolean;
}

export interface GitLog {
  repository: string;
  upstream: string | null;
  upstream_commit: string | null;
  commits: GitCommit[];
}

export interface GitCommitFile { path: string; status: string; original_path: string | null }

export interface GitCommitFiles { repository: string; commit: string; files: GitCommitFile[] }

export interface GitIdentity { name: string; email: string; date: string }

export interface GitCommitStats { files_changed: number; insertions: number; deletions: number; binary_files: number }

export interface GitCommitDetail {
  repository: string;
  id: string;
  short_id: string;
  author: GitIdentity;
  committer: GitIdentity;
  subject: string;
  body: string;
  parents: string[];
  refs: string[];
  stats: GitCommitStats;
  upstream: string | null;
  remote_name: string | null;
  remote_url: string | null;
  commit_url: string | null;
}

export interface GitDiff { repository: string; path: string | null; staged: boolean; patch: string; truncated: boolean; original: string | null; modified: string | null }
