export interface PullRequestUser { id: string; username: string; display_name: string }

export interface ReviewIdentity { provider?: string; remote?: string; provider_name?: string }

export interface ReviewTarget { remote: string; host: string; repository: string; provider: string; provider_name: string }

export interface PullRequestSummary extends ReviewIdentity {
  number: number;
  status: string;
  draft: boolean;
  title: string;
  source_branch: string;
  target_branch: string;
  url: string | null;
  created_at: string;
  updated_at: string;
}

export interface MyPullRequests extends ReviewIdentity {
  repository: string;
  current_branch: string;
  current_user: PullRequestUser;
  pull_requests: PullRequestSummary[];
}

export interface PullRequestCheck {
  id?: string;
  name: string;
  status: string;
  conclusion: string;
  details_url: string | null;
  description?: string;
  text?: string;
  required?: boolean;
  started_at?: string | null;
  completed_at?: string | null;
}

export interface PullRequestComment { id: string; author: string; body: string; created_at: string; updated_at: string }

export interface PullRequestThread {
  id: string;
  path: string | null;
  line: number | null;
  status: string;
  author: string;
  body: string;
  updated_at: string | null;
  comments?: PullRequestComment[];
}

export interface PullRequestFile { path: string; change_type: string; additions?: number | null; deletions?: number | null }

export interface PullRequestDetail extends PullRequestSummary {
  description: string;
  changes_count: number;
  commits_count: number;
  review_status: string;
  check_summary_status: string;
  mergeable: boolean | null;
  reviewers: PullRequestUser[];
  checks: PullRequestCheck[];
  unresolved_threads: PullRequestThread[];
  files: PullRequestFile[];
  author?: PullRequestUser | null;
  labels?: string[];
  merge_checks?: { name: string; passed: boolean | null; reason: string }[];
  threads?: PullRequestThread[];
  warnings?: string[];
  diverged_commits_count?: number;
  milestone?: string | null;
}

export interface PullRequestDiff {
  repository: string;
  number: number;
  path: string;
  original_path: string | null;
  original: string | null;
  modified: string | null;
  patch?: string | null;
  binary: boolean;
  truncated: boolean;
}

export interface ReviewProvider {
  id: string; name: string; enabled: boolean; hosts: string[]; script: string;
}

export interface ReviewProviderSettings { revision: number; providers: ReviewProvider[] }
