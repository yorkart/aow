// AoW Git platform provider v4. Run with Node.js; authentication is provided by glab.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const VERSION = 4;
const CONTENT_LIMIT = 1024 * 1024;
const encodedPath = value => value.split('/').map(encodeURIComponent).join('/');
const user = value => ({ id: String(value?.id ?? ''), username: value?.username ?? '', display_name: value?.name || value?.username || '' });
const summary = mr => ({ number: mr.iid, status: mr.state === 'opened' ? 'open' : mr.state,
  draft: mr.draft ?? mr.work_in_progress ?? /^(Draft|WIP):/i.test(mr.title), title: mr.title,
  source_branch: mr.source_branch, target_branch: mr.target_branch, url: mr.web_url,
  created_at: mr.created_at, updated_at: mr.updated_at });
const comment = note => ({ id: String(note.id), author: note.author?.username ?? '', body: note.body ?? '',
  created_at: note.created_at ?? '', updated_at: note.updated_at ?? '' });

function command(executable, args, options = {}) {
  try {
    return execFileSync(executable, args, { encoding: 'utf8', timeout: 45_000, maxBuffer: 16 * 1024 * 1024,
      stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, GLAB_SEND_TELEMETRY: 'false', GLAB_PAGER: 'cat', GIT_TERMINAL_PROMPT: '0' }, ...options });
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


export class GitLab {
  constructor(request) {
    this.request = request;
    this.repo = request.repository;
    this.prefix = `projects/${encodeURIComponent(this.repo.path)}`;
  }
  api(endpoint, pages = false) {
    const read = path => JSON.parse(command('glab', ['api', '--method', 'GET', path], {
      env: { ...process.env, GITLAB_HOST: this.repo.host, GLAB_SEND_TELEMETRY: 'false', GLAB_PAGER: 'cat' },
    }));
    if (!pages) return read(endpoint);
    const result = [];
    for (let page = 1; ; page++) {
      const values = read(`${endpoint}${endpoint.includes('?') ? '&' : '?'}per_page=100&page=${page}`);
      if (!Array.isArray(values)) throw new Error('GitLab did not return a paginated list');
      result.push(...values);
      if (values.length < 100) return result;
    }
  }
  repositoryInfo() {
    const project = this.api(this.prefix);
    const avatar = project.namespace?.avatar_url;
    return { avatar_url: avatar ? new URL(avatar, `https://${this.repo.host}`).href : null };
  }
  commitLinks(commit) {
    const remote_url = `https://${this.repo.host}/${encodedPath(this.repo.path)}`;
    return { remote_url, commit_url: `${remote_url}/-/commit/${encodeURIComponent(commit)}` };
  }
  list() {
    const state = this.request.params?.state ?? 'open';
    if (!['open', 'all'].includes(state)) throw Object.assign(new Error('list state must be open or all'), { code: 'invalid_request' });
    const current = this.api('user');
    const items = this.api(`${this.prefix}/merge_requests?scope=all&author_id=${encodeURIComponent(current.id)}&order_by=updated_at&sort=desc${state === 'open' ? '&state=opened' : ''}`, true);
    return { repository: this.repo.root, current_branch: command('git', ['branch', '--show-current']).trim() || 'HEAD',
      current_user: user(current), pull_requests: items.filter(mr => mr.author?.id === current.id).map(summary) };
  }
  files(number) { return this.api(`${this.prefix}/merge_requests/${number}/diffs`, true); }
  threads(number) {
    return this.api(`${this.prefix}/merge_requests/${number}/discussions`, true).flatMap(discussion => {
      const notes = (discussion.notes ?? []).filter(note => !note.system);
      if (!notes.length) return [];
      const first = notes[0];
      const position = first.position;
      const resolvable = notes.filter(note => note.resolvable);
      const status = resolvable.length ? resolvable.every(note => note.resolved) ? 'resolved' : 'open' : 'comment';
      const comments = notes.map(comment);
      return [{ id: String(discussion.id), path: position?.new_path ?? position?.old_path ?? null,
        line: position?.new_line ?? position?.old_line ?? null, status, author: comments[0].author, body: comments[0].body,
        updated_at: comments.at(-1).updated_at, comments }];
    });
  }
  detail(number) {
    const endpoint = `${this.prefix}/merge_requests/${number}`;
    const mr = this.api(endpoint);
    const warnings = [];
    const section = (label, read, fallback) => {
      try { return read(); } catch (error) { warnings.push(`${label}: ${error.message}`); return fallback; }
    };
    const files = section('Changed files', () => this.files(number), []);
    const threads = section('Discussions', () => this.threads(number), []);
    const commits = section('Commits', () => this.api(`${endpoint}/commits`, true), null);
    const approvals = section('Approvals', () => this.api(`${endpoint}/approvals`), null);
    let pipeline = mr.head_pipeline;
    if (!pipeline) pipeline = section('Pipelines', () => this.api(`${endpoint}/pipelines`, true).find(value => value.sha === mr.sha) ?? null, null);
    const jobs = pipeline ? section('Pipeline jobs', () => this.api(`projects/${encodeURIComponent(pipeline.project_id ?? mr.source_project_id ?? mr.target_project_id ?? this.repo.path)}/pipelines/${pipeline.id}/jobs`, true), []) : [];
    const checks = jobs.map(job => ({ id: String(job.id), name: job.name ?? '', status: job.status ?? 'unknown',
      conclusion: ['success', 'failed', 'canceled', 'skipped'].includes(job.status) ? job.status : '',
      details_url: job.web_url ?? null, description: job.failure_reason ?? '', text: '', required: !job.allow_failure,
      started_at: job.started_at ?? null, completed_at: job.finished_at ?? null }));
    const pipelineStatus = pipeline?.status;
    const ci = pipelineStatus === 'success' ? 'passed' : ['failed', 'canceled'].includes(pipelineStatus) ? 'failed'
      : ['created', 'waiting_for_resource', 'preparing', 'pending', 'running', 'manual', 'scheduled'].includes(pipelineStatus) ? 'pending' : 'unknown';
    const mergeState = mr.detailed_merge_status ?? mr.merge_status ?? 'unknown';
    const unknown = ['unknown', 'unchecked', 'checking', 'preparing', 'approvals_syncing'].includes(mergeState);
    const draft = summary(mr).draft;
    const mergeable = unknown ? null : ['mergeable', 'can_be_merged'].includes(mergeState) && !draft;
    const count = Number.parseInt(mr.changes_count, 10);
    if ((Number.isFinite(count) && files.length < count) || String(mr.changes_count).endsWith('+')) warnings.push(`File list may be incomplete: ${files.length}/${mr.changes_count}.`);
    if (files.some(file => file.too_large || file.collapsed)) warnings.push('Some file patches are collapsed or too large in GitLab.');
    return { ...summary(mr), description: mr.description ?? '', changes_count: Number.isFinite(count) ? count : files.length,
      commits_count: commits?.length ?? 0, review_status: approvals ? approvals.approved === true || approvals.approvals_left === 0 ? 'approved' : 'review_required' : 'unknown',
      check_summary_status: ci, mergeable, reviewers: (mr.reviewers ?? []).map(user), checks,
      unresolved_threads: threads.filter(value => value.status === 'open'), files: files.map(file => {
        const complete = typeof file.diff === 'string' && !file.too_large && !file.collapsed;
        const lines = complete ? file.diff.split('\n') : [];
        return { path: file.new_path, change_type: file.new_file ? 'A' : file.deleted_file ? 'D' : file.renamed_file ? 'R' : 'M',
          additions: complete ? lines.filter(line => line.startsWith('+') && !line.startsWith('+++')).length : null,
          deletions: complete ? lines.filter(line => line.startsWith('-') && !line.startsWith('---')).length : null };
      }), author: mr.author ? user(mr.author) : null, labels: mr.labels ?? [], milestone: mr.milestone?.title ?? null,
      diverged_commits_count: mr.diverged_commits_count ?? 0, merge_checks: [{ name: 'merge_state', passed: mergeable, reason: mergeState }],
      threads, warnings };
  }
  content(project, path, ref) {
    if (!project || !ref) throw new Error('GitLab did not provide the project or comparison commit for this file');
    const value = this.api(`projects/${encodeURIComponent(project)}/repository/files/${encodeURIComponent(path)}?ref=${encodeURIComponent(ref)}`);
    if ((value.size ?? 0) > CONTENT_LIMIT || value.encoding !== 'base64') return { text: null, binary: false, truncated: true };
    const raw = Buffer.from(value.content, 'base64');
    if (raw.length > CONTENT_LIMIT) return { text: null, binary: false, truncated: true };
    try {
      if (raw.includes(0)) return { text: null, binary: true, truncated: false };
      return { text: new TextDecoder('utf-8', { fatal: true }).decode(raw), binary: false, truncated: false };
    } catch { return { text: null, binary: true, truncated: false }; }
  }
  diff(number, path, patchOnly = false) {
    const mr = this.api(`${this.prefix}/merge_requests/${number}`);
    const file = this.files(number).find(value => value.new_path === path);
    if (!file) throw Object.assign(new Error('File is not part of this merge request'), { code: 'invalid_request' });
    const oldPath = file.old_path;
    let original = { text: '', binary: false, truncated: false };
    let modified = { ...original };
    if (!file.new_file) original = this.content(mr.target_project_id ?? this.repo.path, oldPath, mr.diff_refs?.base_sha);
    if (!file.deleted_file) modified = this.content(mr.source_project_id ?? mr.target_project_id ?? this.repo.path, path, mr.diff_refs?.head_sha ?? mr.sha);
    const binary = original.binary || modified.binary;
    const truncated = original.truncated || modified.truncated;
    return { repository: this.repo.root, number, path, original_path: oldPath === path ? null : oldPath,
      original: patchOnly ? null : original.text, modified: patchOnly ? null : modified.text,
      patch: binary || truncated ? null : unifiedDiff(original.text, modified.text, oldPath, path), binary, truncated };
  }
}

export function dispatch(request) {
  if (request.version !== VERSION) throw Object.assign(new Error('Unsupported protocol version'), { code: 'unsupported_version' });
  if (request.operation === 'describe') return { name: 'GitLab', hosts: ['gitlab.com'], operations: ['pull_requests.list', 'pull_requests.detail', 'pull_requests.diff', 'repository.info', 'repository.commit_links'] };
  if (!['pull_requests.list', 'pull_requests.detail', 'pull_requests.diff', 'repository.info', 'repository.commit_links'].includes(request.operation)) throw Object.assign(new Error(`Unsupported operation: ${request.operation}`), { code: 'unsupported_operation' });
  const client = new GitLab(request);
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
