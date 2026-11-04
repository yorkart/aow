// AoW Git platform provider v4. Run with Node.js; authentication is provided by gh.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const VERSION = 4;
const CONTENT_LIMIT = 1024 * 1024;
const COMMENT_FIELDS = 'id body createdAt updatedAt author { login }';
const PAGE_INFO = 'pageInfo { hasNextPage endCursor }';
const encodedPath = value => value.split('/').map(encodeURIComponent).join('/');
const user = value => ({ id: String(value?.id ?? ''), username: value?.login ?? '', display_name: value?.name || value?.login || '' });
const summary = pr => ({ number: pr.number, status: pr.merged_at ? 'merged' : pr.state, draft: pr.draft ?? false,
  title: pr.title, source_branch: pr.head.ref, target_branch: pr.base.ref, url: pr.html_url,
  created_at: pr.created_at, updated_at: pr.updated_at });
const comment = value => ({ id: String(value.id), author: (value.author ?? value.user)?.login ?? '', body: value.body ?? '',
  created_at: value.createdAt ?? value.created_at ?? '', updated_at: value.updatedAt ?? value.updated_at ?? '' });
const thread = (id, comments, path = null, line = null, status = 'comment') => ({ id: String(id), path, line, status,
  author: comments[0]?.author ?? '', body: comments[0]?.body ?? '', updated_at: comments.at(-1)?.updated_at ?? null, comments });

function command(executable, args, options = {}) {
  try {
    return execFileSync(executable, args, { encoding: 'utf8', timeout: 45_000, maxBuffer: 16 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, GH_PROMPT_DISABLED: '1', GH_PAGER: 'cat', GIT_TERMINAL_PROMPT: '0' }, ...options });
  } catch (error) {
    throw Object.assign(new Error(String(error.stderr || error.message).trim()), { code: error.code === 'ENOENT' ? 'dependency_missing' : 'upstream_error' });
  }
}

function unifiedDiff(original, modified, oldPath, newPath) {
  if (original === modified) return '';
  const directory = mkdtempSync(join(tmpdir(), 'aow-pull-request-diff-'));
  try {
    const before = join(directory, 'before');
    const after = join(directory, 'after');
    writeFileSync(before, original);
    writeFileSync(after, modified);
    let output;
    try {
      output = execFileSync('git', ['diff', '--no-index', '--no-ext-diff', '--no-textconv', '--no-color', '--text', '--', before, after],
        { encoding: 'utf8', timeout: 15_000, maxBuffer: 16 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (error) {
      if (error.status !== 1) throw error;
      output = error.stdout;
    }
    const start = output.indexOf('@@ ');
    if (start < 0) throw new Error('Git did not return a text diff');
    return `--- a/${oldPath}\n+++ b/${newPath}\n${output.slice(start)}`;
  } finally { rmSync(directory, { recursive: true, force: true }); }
}

export class GitHub {
  constructor(request) {
    this.request = request;
    this.repo = request.repository;
    this.prefix = `repos/${encodedPath(this.repo.path)}`;
  }
  command(...args) { return JSON.parse(command('gh', args)); }
  api(endpoint, pages = false) {
    const result = this.command('api', '--hostname', this.repo.host, endpoint, ...(pages ? ['--paginate', '--slurp'] : []));
    return pages ? result.flat() : result;
  }
  graphql(query, variables) {
    const args = ['api', '--hostname', this.repo.host, 'graphql', '-f', `query=${query}`];
    for (const [key, value] of Object.entries(variables)) {
      if (value != null) args.push(typeof value === 'number' ? '-F' : '-f', `${key}=${value}`);
    }
    const result = this.command(...args);
    if (result.errors?.length) throw new Error(result.errors.map(error => error.message).join('; '));
    return result.data;
  }
  repositoryInfo() { return { avatar_url: this.api(this.prefix).owner?.avatar_url || null }; }
  commitLinks(commit) {
    const remote_url = `https://${this.repo.host}/${encodedPath(this.repo.path)}`;
    return { remote_url, commit_url: `${remote_url}/commit/${encodeURIComponent(commit)}` };
  }
  list() {
    const state = this.request.params?.state ?? 'open';
    if (!['open', 'all'].includes(state)) throw Object.assign(new Error('list state must be open or all'), { code: 'invalid_request' });
    const current = this.api('user');
    const items = this.api(`${this.prefix}/pulls?state=${state}&sort=updated&direction=desc&per_page=100`, true);
    const branch = command('git', ['branch', '--show-current']).trim();
    return { repository: this.repo.root, current_branch: branch || 'HEAD', current_user: user(current),
      pull_requests: items.filter(pr => pr.user.id === current.id).map(summary) };
  }
  files(number) { return this.api(`${this.prefix}/pulls/${number}/files?per_page=100`, true); }
  reviewThreads(number) {
    const [owner, ...name] = this.repo.path.split('/');
    const variables = { owner, name: name.join('/'), number };
    const query = `query($owner:String!,$name:String!,$number:Int!,$cursor:String) {
      repository(owner:$owner,name:$name) { pullRequest(number:$number) {
        reviewThreads(first:100,after:$cursor) { nodes { id path line isResolved
          comments(first:100) { nodes { ${COMMENT_FIELDS} } ${PAGE_INFO} } } ${PAGE_INFO} }
      } } }`;
    const result = [];
    for (;;) {
      const connection = this.graphql(query, variables).repository.pullRequest.reviewThreads;
      for (const item of connection.nodes) {
        const comments = [...item.comments.nodes];
        let page = item.comments.pageInfo;
        while (page.hasNextPage) {
          const extra = this.graphql(`query($id:ID!,$cursor:String) {
            node(id:$id) { ... on PullRequestReviewThread {
              comments(first:100,after:$cursor) { nodes { ${COMMENT_FIELDS} } ${PAGE_INFO} }
            } } }`, { id: item.id, cursor: page.endCursor }).node.comments;
          comments.push(...extra.nodes);
          page = extra.pageInfo;
        }
        result.push(thread(item.id, comments.map(comment), item.path, item.line, item.isResolved ? 'resolved' : 'open'));
      }
      if (!connection.pageInfo.hasNextPage) return result;
      variables.cursor = connection.pageInfo.endCursor;
    }
  }
  detail(number) {
    const pr = this.api(`${this.prefix}/pulls/${number}`);
    const warnings = [];
    const section = (label, read, fallback) => {
      try { return read(); } catch (error) { warnings.push(`${label}: ${error.message}`); return fallback; }
    };
    const files = section('Changed files', () => this.files(number), []);
    if (files.length < pr.changed_files) warnings.push(`File list incomplete: ${files.length}/${pr.changed_files}; GitHub returns at most 3000 files.`);
    const metadata = section('Reviews and checks', () => this.command('pr', 'view', String(number), '--repo', `${this.repo.host}/${this.repo.path}`,
      '--json', 'reviewDecision,statusCheckRollup,mergeStateStatus'), {});
    const checks = (metadata.statusCheckRollup ?? []).map((check, index) => ({ id: String(check.databaseId ?? index),
      name: check.name ?? check.context ?? '', status: (check.status || check.state || 'unknown').toLowerCase(),
      conclusion: (check.conclusion || check.state || '').toLowerCase(), details_url: check.detailsUrl ?? check.targetUrl ?? null,
      description: check.description ?? '', text: '', required: false, started_at: check.startedAt ?? null, completed_at: check.completedAt ?? null }));
    const statuses = checks.map(check => check.conclusion || check.status);
    const ci = statuses.some(status => ['failure', 'error', 'cancelled', 'timed_out', 'action_required', 'startup_failure'].includes(status)) ? 'failed'
      : statuses.length && statuses.every(status => ['success', 'neutral', 'skipped'].includes(status)) ? 'passed' : statuses.length ? 'pending' : 'unknown';
    const threads = section('Review discussions', () => this.reviewThreads(number), []);
    const general = section('General discussion', () => this.api(`${this.prefix}/issues/${number}/comments?per_page=100`, true), []);
    threads.push(...general.map(value => thread(`issue-${value.id}`, [comment(value)])));
    const mergeState = metadata.mergeStateStatus || 'UNKNOWN';
    const mergeable = mergeState === 'UNKNOWN' ? null : mergeState === 'CLEAN' && !pr.draft;
    return { ...summary(pr), description: pr.body ?? '', changes_count: pr.changed_files, commits_count: pr.commits,
      review_status: (metadata.reviewDecision || 'unknown').toLowerCase(), check_summary_status: ci, mergeable,
      reviewers: (pr.requested_reviewers ?? []).map(user), checks, unresolved_threads: threads.filter(value => value.status === 'open'),
      files: files.map(file => ({ path: file.filename, change_type: ({ added: 'A', removed: 'D', renamed: 'R', copied: 'C' })[file.status] ?? 'M',
        additions: file.additions ?? null, deletions: file.deletions ?? null })), author: user(pr.user), labels: (pr.labels ?? []).map(label => label.name),
      merge_checks: [{ name: 'merge_state', passed: mergeable, reason: mergeState }], threads, warnings, diverged_commits_count: 0,
      milestone: pr.milestone?.title ?? null };
  }
  content(repository, path, ref) {
    const value = this.api(`repos/${encodedPath(repository)}/contents/${encodedPath(path)}?ref=${encodeURIComponent(ref)}`);
    if (Array.isArray(value) || (value.size ?? 0) > CONTENT_LIMIT || value.encoding !== 'base64') return { text: null, binary: false, truncated: true };
    const raw = Buffer.from(value.content, 'base64');
    if (raw.length > CONTENT_LIMIT) return { text: null, binary: false, truncated: true };
    try {
      if (raw.includes(0)) return { text: null, binary: true, truncated: false };
      return { text: new TextDecoder('utf-8', { fatal: true }).decode(raw), binary: false, truncated: false };
    } catch { return { text: null, binary: true, truncated: false }; }
  }
  diff(number, path, patchOnly = false) {
    const pr = this.api(`${this.prefix}/pulls/${number}`);
    const file = this.files(number).find(value => value.filename === path);
    if (!file) throw Object.assign(new Error('File is not part of this pull request'), { code: 'invalid_request' });
    const oldPath = file.previous_filename ?? path;
    let original = { text: '', binary: false, truncated: false };
    let modified = { ...original };
    if (file.status !== 'added') {
      const comparison = this.api(`${this.prefix}/compare/${pr.base.sha}...${pr.head.sha}?per_page=1`);
      original = this.content(pr.base.repo.full_name, oldPath, comparison.merge_base_commit.sha);
    }
    if (file.status !== 'removed') modified = this.content(this.repo.path, path, pr.head.sha);
    const binary = original.binary || modified.binary;
    const truncated = original.truncated || modified.truncated;
    return { repository: this.repo.root, number, path, original_path: oldPath === path ? null : oldPath,
      original: patchOnly ? null : original.text, modified: patchOnly ? null : modified.text,
      patch: binary || truncated ? null : unifiedDiff(original.text, modified.text, oldPath, path), binary, truncated };
  }
}

export function dispatch(request) {
  if (request.version !== VERSION) throw Object.assign(new Error('Unsupported protocol version'), { code: 'unsupported_version' });
  if (request.operation === 'describe') return { name: 'GitHub', hosts: ['github.com'], operations: ['pull_requests.list', 'pull_requests.detail', 'pull_requests.diff', 'repository.info', 'repository.commit_links'] };
  const allowed = ['pull_requests.list', 'pull_requests.detail', 'pull_requests.diff', 'repository.info', 'repository.commit_links'];
  if (!allowed.includes(request.operation)) throw Object.assign(new Error(`Unsupported operation: ${request.operation}`), { code: 'unsupported_operation' });
  const client = new GitHub(request);
  const params = request.params ?? {};
  if (request.operation === 'pull_requests.list') return client.list();
  if (request.operation === 'pull_requests.detail') return client.detail(params.number);
  if (request.operation === 'pull_requests.diff') return client.diff(params.number, params.path, params.patch_only);
  if (request.operation === 'repository.info') return client.repositoryInfo();
  return client.commitLinks(params.commit);
}

if (process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)) {
  let response;
  try { response = { version: VERSION, result: dispatch(JSON.parse(readFileSync(0, 'utf8'))) }; }
  catch (error) { response = { version: VERSION, error: { code: error.code || 'upstream_error', message: error.message } }; }
  process.stdout.write(`${JSON.stringify(response)}\n`);
}
