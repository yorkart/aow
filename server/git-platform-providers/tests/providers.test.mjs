import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { GitHub, dispatch as github } from '../github.mjs';
import { GitLab, dispatch as gitlab } from '../gitlab.mjs';

const request = { version: 4, operation: 'pull_requests.list', repository: { root: '/workspace/project', host: 'git.example.com', path: 'team/project', remote: 'upstream' }, params: {} };
const pr = { number: 42, state: 'open', draft: false, title: 'Example', body: 'Description',
  head: { ref: 'feature', sha: 'head-sha', repo: null }, base: { ref: 'main', sha: 'base-sha', repo: { full_name: 'team/project' } },
  html_url: 'https://git.example.com/team/project/pull/42', created_at: '', updated_at: '', user: { id: 1, login: 'alice' }, changed_files: 1, commits: 2, labels: [] };
const mr = { iid: 42, state: 'opened', draft: false, title: 'Example', description: 'Description',
  source_branch: 'feature', target_branch: 'main', source_project_id: 11, target_project_id: 10,
  diff_refs: { base_sha: 'base-sha', head_sha: 'head-sha' }, sha: 'head-sha', changes_count: '1',
  web_url: 'https://git.example.com/team/project/-/merge_requests/42', created_at: '', updated_at: '', author: { id: 1, username: 'alice' } };
const note = { id: 'comment', author: { login: 'alice' }, body: 'hello', createdAt: '', updatedAt: '' };
const text = value => ({ text: value, binary: false, truncated: false });

for (const [name, dispatch, Client] of [['github', github, GitHub], ['gitlab', gitlab, GitLab]]) {
  test(`${name}: standalone describe and links need no platform CLI, including a symlinked script path`, t => {
    const directory = mkdtempSync(join(tmpdir(), 'aow-provider-test-'));
    t.after(() => rmSync(directory, { recursive: true, force: true }));
    const script = join(directory, `${name}.mjs`);
    symlinkSync(fileURLToPath(new URL(`../${name}.mjs`, import.meta.url)), script);
    const run = input => {
      const result = spawnSync(process.execPath, [script], { input: JSON.stringify(input), encoding: 'utf8', env: { PATH: '' } });
      assert.equal(result.status, 0, result.stderr);
      return JSON.parse(result.stdout);
    };
    const description = run({ version: 4, operation: 'describe', repository: null, params: {} });
    assert.equal(description.version, 4);
    assert.deepEqual(description.result.hosts, [`${name}.com`]);
    assert.deepEqual(description.result.operations, ['pull_requests.list', 'pull_requests.detail', 'pull_requests.diff', 'repository.info', 'repository.commit_links']);
    const links = run({ ...request, operation: 'repository.commit_links', params: { commit: 'a'.repeat(40) } }).result;
    assert.equal(links.remote_url, 'https://git.example.com/team/project');
    assert.equal(links.commit_url, `https://git.example.com/team/project/${name === 'gitlab' ? '-/' : ''}commit/${'a'.repeat(40)}`);
    for (const version of [2, 3]) assert.equal(run({ ...request, version }).error.code, 'unsupported_version');
    for (const operation of ['list', 'detail', 'diff', 'repository_info', 'commit_links', 'merge', 'create', 'approve', 'comment', 'resolve', 'pull_requests.merge']) {
      assert.equal(run({ ...request, operation }).error.code, 'unsupported_operation');
      assert.throws(() => dispatch({ ...request, operation }), { code: 'unsupported_operation' });
    }
  });

  test(`${name}: list author filtering, all states and failures`, () => {
    const client = new Client({ ...request, params: { state: 'all' } });
    const calls = [];
    client.api = (endpoint, pages) => {
      calls.push([endpoint, pages]);
      if (endpoint === 'user') return { id: 1, login: 'alice', username: 'alice' };
      const initial = name === 'github' ? pr : mr;
      return [initial, { ...initial, number: 43, iid: 43, state: 'closed', merged_at: null },
        { ...initial, number: 44, iid: 44, state: name === 'github' ? 'closed' : 'merged', merged_at: 'today' },
        { ...initial, user: { id: 2 }, author: { id: 2 } }];
    };
    const result = client.list();
    assert.deepEqual(result.pull_requests.map(value => [value.number, value.status]), [[42, 'open'], [43, 'closed'], [44, 'merged']]);
    assert.equal(calls[1][1], true);
    assert.equal(result.current_user.username, 'alice');
    if (name === 'github') assert.match(calls[1][0], /state=all/);
    else { assert.match(calls[1][0], /author_id=1/); assert.doesNotMatch(calls[1][0], /state=/); }
    client.request.params = { state: 'invalid' };
    assert.throws(() => client.list(), /list state/);
    client.request.params = {};
    client.api = () => { throw new Error('auth required'); };
    assert.throws(() => client.list(), /auth required/);
  });

  test(`${name}: renamed-file diff uses the comparison base and preserves missing final newline`, () => {
    const client = new Client(request);
    const calls = [];
    client.api = endpoint => endpoint.includes('/compare/') ? { merge_base_commit: { sha: 'merge-base' } } : name === 'github' ? pr : { ...mr, source_project_id: null };
    client.files = () => name === 'github' ? [{ filename: 'new name', previous_filename: 'old name', status: 'renamed' }] : [{ new_path: 'new name', old_path: 'old name', renamed_file: true }];
    client.content = (...args) => { calls.push(args); return text(calls.length % 2 ? 'before' : 'after'); };
    const result = client.diff(42, 'new name');
    assert.deepEqual(calls, name === 'github' ? [['team/project', 'old name', 'merge-base'], ['team/project', 'new name', 'head-sha']] : [[10, 'old name', 'base-sha'], [10, 'new name', 'head-sha']]);
    assert.equal(result.original_path, 'old name');
    assert.match(result.patch, /--- a\/old name\n\+\+\+ b\/new name/);
    assert.match(result.patch, /No newline at end of file/);
    assert.equal(client.diff(42, 'new name', true).original, null);
    assert.throws(() => client.diff(42, 'missing'), /not part of/);
  });

  test(`${name}: added, removed, binary, oversized and unchanged file content`, () => {
    const client = new Client(request);
    client.api = endpoint => endpoint.includes('/compare/') ? { merge_base_commit: { sha: 'base' } } : name === 'github' ? pr : mr;
    for (const status of ['added', 'removed', 'modified']) {
      client.files = () => name === 'github' ? [{ filename: 'file', status }] : [{ new_path: 'file', old_path: 'file', new_file: status === 'added', deleted_file: status === 'removed' }];
      client.content = () => text('content\n');
      const result = client.diff(42, 'file');
      assert.equal(result.original, status === 'added' ? '' : 'content\n');
      assert.equal(result.modified, status === 'removed' ? '' : 'content\n');
      if (status === 'modified') assert.equal(result.patch, '');
      for (const flag of ['binary', 'truncated']) {
        client.content = () => ({ ...text(null), [flag]: true });
        const limited = client.diff(42, 'file', true);
        assert.equal(limited[flag], true);
        assert.equal(limited.patch, null);
      }
    }
    client.api = () => ({ encoding: 'base64', content: Buffer.from([0]).toString('base64') });
    client.content = Client.prototype.content;
    assert.equal(client.content('project', 'dir/file name', 'ref').binary, true);
    client.api = endpoint => { assert.match(endpoint, /ref=ref%20name/); return { encoding: 'base64', size: 1024 * 1024 + 1 }; };
    assert.equal(client.content('project', 'file', 'ref name').truncated, true);
    client.api = () => { throw new Error('not found'); };
    assert.throws(() => client.content('project', 'file', 'ref'), /not found/);
  });
}

test('GitHub pagination targets the host explicitly and paginates threads and their comments', () => {
  const client = new GitHub(request);
  client.command = (...args) => {
    assert.deepEqual(args, ['api', '--hostname', 'git.example.com', 'endpoint', '--paginate', '--slurp']);
    return [[1, 2], [3]];
  };
  assert.deepEqual(client.api('endpoint', true), [1, 2, 3]);
  const connection = (nodes, next = false, cursor = null) => ({ nodes, pageInfo: { hasNextPage: next, endCursor: cursor } });
  const responses = [
    { repository: { pullRequest: { reviewThreads: connection([{ id: 't1', path: 'file', line: 1, isResolved: false, comments: connection([note], true, 'comments-2') }], true, 'threads-2') } } },
    { node: { comments: connection([{ ...note, id: 'reply' }]) } },
    { repository: { pullRequest: { reviewThreads: connection([{ id: 't2', path: 'file', line: 2, isResolved: true, comments: connection([note]) }]) } } },
  ];
  const variables = [];
  client.graphql = (_, values) => { variables.push({ ...values }); return responses.shift(); };
  const threads = client.reviewThreads(42);
  assert.deepEqual(threads.map(value => [value.status, value.comments.length]), [['open', 2], ['resolved', 1]]);
  assert.equal(variables[1].cursor, 'comments-2');
  assert.equal(variables[2].cursor, 'threads-2');
});

test('GitHub detail reports incomplete optional sections and preserves primary failures', () => {
  const client = new GitHub(request);
  client.api = endpoint => endpoint.endsWith('/pulls/42') ? pr : (() => { throw new Error('comments unavailable'); })();
  for (const method of ['files', 'command', 'reviewThreads']) client[method] = () => { throw new Error(`${method} unavailable`); };
  const result = client.detail(42);
  assert.ok(result.warnings.length >= 4);
  assert.equal(result.check_summary_status, 'unknown');
  assert.equal(result.mergeable, null);
  client.api = () => { throw new Error('auth required'); };
  assert.throws(() => client.detail(42), /auth required/);
});

test('GitLab CLI reads use GET, the selected host and all pages', t => {
  const directory = mkdtempSync(join(tmpdir(), 'aow-glab-test-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const cli = join(directory, 'glab');
  const log = join(directory, 'calls.jsonl');
  writeFileSync(cli, `#!${process.execPath}\nimport {appendFileSync} from 'node:fs';
const args=process.argv.slice(2); appendFileSync(${JSON.stringify(log)}, JSON.stringify({args,host:process.env.GITLAB_HOST})+'\\n');
console.log(JSON.stringify(Array.from({length:args.at(-1).endsWith('page=1')?100:2},(_,id)=>({id}))));\n`, { mode: 0o755 });
  const script = `import {GitLab} from ${JSON.stringify(new URL('../gitlab.mjs', import.meta.url).href)};
console.log(JSON.stringify(new GitLab(${JSON.stringify(request)}).api('projects/team%2Fproject/merge_requests?state=opened',true)));`;
  const result = execFileSync(process.execPath, ['--input-type=module', '-e', script], { encoding: 'utf8', env: { ...process.env, PATH: directory } });
  assert.equal(JSON.parse(result).length, 102);
  const calls = readFileSync(log, 'utf8').trim().split('\n').map(JSON.parse);
  assert.equal(calls.length, 2);
  for (const [index, call] of calls.entries()) {
    assert.equal(call.host, 'git.example.com');
    assert.deepEqual(call.args.slice(0, 3), ['api', '--method', 'GET']);
    assert.match(call.args.at(-1), new RegExp(`&per_page=100&page=${index + 1}$`));
  }
});

test('GitLab discussion resolution, approvals, CI and partial-file warnings', () => {
  const client = new GitLab(request);
  const notes = [{ id: 1, body: 'review', author: { username: 'alice' }, resolvable: true, resolved: false, position: { old_path: 'file', old_line: 3 } },
    { id: 2, body: 'reply', author: { username: 'bob' }, resolvable: true, resolved: true }];
  client.api = endpoint => {
    if (endpoint.endsWith('/merge_requests/42')) return { ...mr, changes_count: '1000+', detailed_merge_status: 'mergeable', head_pipeline: { id: 7, project_id: 11, status: 'failed' } };
    if (endpoint.endsWith('/discussions')) return [{ id: 't1', notes }, { id: 'system', notes: [{ system: true }] }];
    if (endpoint.endsWith('/approvals')) return { approved: true, approvals_left: 0 };
    if (endpoint.endsWith('/jobs')) return [{ id: 1, name: 'test', status: 'failed', allow_failure: false }];
    if (endpoint.endsWith('/diffs')) return [{ new_path: 'file', old_path: 'file', too_large: true }];
    if (endpoint.endsWith('/commits')) return [{}];
    throw new Error(`unexpected ${endpoint}`);
  };
  const result = client.detail(42);
  assert.equal(result.unresolved_threads[0].comments.length, 2);
  assert.equal(result.unresolved_threads[0].line, 3);
  assert.equal(result.review_status, 'approved');
  assert.equal(result.check_summary_status, 'failed');
  assert.equal(result.checks[0].required, true);
  assert.equal(result.files[0].additions, null);
  assert.equal(result.warnings.length, 2);
  notes[0].resolved = true;
  assert.equal(client.detail(42).unresolved_threads.length, 0);
});

test('repository avatars represent the owner and normalize GitLab relative URLs', () => {
  for (const Client of [GitHub, GitLab]) {
    const client = new Client(request);
    client.api = () => ({ owner: { avatar_url: 'https://images.example.com/team' }, namespace: { avatar_url: '/uploads/team.png' } });
    assert.equal(client.repositoryInfo().avatar_url, Client === GitHub ? 'https://images.example.com/team' : 'https://git.example.com/uploads/team.png');
    client.api = () => ({ avatar_url: 'https://images.example.com/project-logo' });
    assert.equal(client.repositoryInfo().avatar_url, null);
    client.api = () => { throw new Error('auth required'); };
    assert.throws(() => client.repositoryInfo(), /auth required/);
  }
});
