// Run from frontend: node tests/pull-request-preview.mjs
import { createServer } from 'vite';
import { fileURLToPath } from 'node:url';
import { detail } from './fixtures/pull-request.mjs';
const requests = new Map();
const server = await createServer({
  root: fileURLToPath(new URL('../', import.meta.url)),
  server: { host: '127.0.0.1', port: Number(process.env.PR_PREVIEW_PORT || 5199), strictPort: true, proxy: {} },
  plugins: [{ name: 'read-only-pr-fixture', configureServer(server) {
    server.middlewares.use('/api/my-pull-requests', (req, res) => {
      console.log(`${req.method} /api/my-pull-requests${req.url}`);
      res.setHeader('Content-Type', 'application/json');
      if (req.method !== 'GET') { res.statusCode = 405; res.end(JSON.stringify({ message: 'Unexpected mutation' })); return; }
      const url = new URL(req.url, 'http://localhost');
      const scenario = url.searchParams.get('repo')?.split('/').at(-1);
      const count = (requests.get(req.url) || 0) + 1; requests.set(req.url, count);
      if (scenario === 'error' || (scenario === 'refresh-error' && count > 2) || (scenario === 'diff-error' && url.pathname.endsWith('/diff') && count === 1)) {
        res.statusCode = 500; res.end(JSON.stringify({ message: '测试：服务暂时不可用' })); return;
      }
      let result = structuredClone(detail);
      const number = Number(url.pathname.split('/')[1]) || 42;
      result.number = number;
      if (number === 43) Object.assign(result, { title: 'docs: 补充手机站说明', draft: true, source_branch: 'docs/mobile-guide' });
      if (scenario === 'empty') result = { ...result, description: '', files: [], threads: [], unresolved_threads: [], checks: [], reviewers: [], labels: [], merge_checks: [], author: null, commits_count: 0, changes_count: 0, mergeable: null, check_summary_status: 'no_checks', review_status: 'unknown' };
      if (scenario === 'readonly') result.description = '<input type=checkbox checked> Task <input type=text value=editable><form><button>Submit</button></form><div contenteditable=true>Read only HTML</div>';
      if (scenario === 'partial') result.warnings = ['CI 检查加载失败：服务超时，请刷新重试。'];
      if (scenario === 'long') { result.title = 'feat: ' + '兼容非常长的Pull Request标题和各种狭窄工作台布局'.repeat(4); result.source_branch = 'feature/' + 'very-long-branch-name-'.repeat(12); }
      if (url.pathname === '/' || url.pathname === '') result = { repository: url.searchParams.get('repo'), current_branch: 'feature/pr-details', current_user: detail.author, pull_requests: scenario === 'empty' ? [] : [detail, { ...detail, number: 43, title: 'docs: 补充手机站说明', draft: true, source_branch: 'docs/mobile-guide' }] };
      if (url.pathname.endsWith('/diff')) {
        const path = url.searchParams.get('path');
        result = { repository: '/fixtures/' + scenario, number, path, original_path: null, original: 'const options = { readOnly: false };\n', modified: 'const options = { readOnly: true };\nconst label = "只读";\n', binary: path?.endsWith('.png'), truncated: false, patch: '@@ -1 +1,2 @@\n-const options = { readOnly: false };\n+const options = { readOnly: true };\n+const label = "只读";\n' };
      }
      if (url.pathname.endsWith('/diff') && url.searchParams.get('patch_only') === 'true') { result.original = null; result.modified = null; }
      res.end(JSON.stringify(result));
    });
    // A full mobile-app fixture. Unknown routes never reach a live AoW service.
    server.middlewares.use('/api', (req, res) => {
      console.log(`${req.method} /api${req.url}`);
      res.setHeader('Content-Type', 'application/json');
      if (req.method !== 'GET') { res.statusCode = 405; res.end(JSON.stringify({ message: 'Unexpected mutation' })); return; }
      const path = new URL(req.url, 'http://localhost').pathname;
      const project = { id: 'pr-project', name: 'AoW', registered_path: '/fixtures/default', common_git_dir: '/fixtures/default/.git', notes_path: '/notes', worktrees: ['default', 'empty', 'error'].map((name) => ({ id: name, project_id: 'pr-project', path: '/fixtures/' + name, branch: name === 'default' ? 'feature/pr-details' : name, head: 'abc', is_main: name === 'default', detached: false, locked: false, prunable: false, color: 'default' })) };
      const data = { '/review-targets': [], '/auth/status': { configured: true, authenticated: true }, '/aow/projects': [project], '/aow/pinned-worktrees': { paths: [], revision: 0 }, '/aow/settings': { notes_base: '/notes' }, '/aow/agents': [], '/terminals': [], '/aow/agent-sessions': [], '/aow/automations': [], '/git/status': { repository: '/fixtures/default', branch: 'feature/pr-details', files: [], ahead: 0, behind: 0 } };
      if (!(path in data)) { res.statusCode = 404; res.end(JSON.stringify({ message: 'No test fixture: ' + path })); return; }
      res.end(JSON.stringify(data[path]));
    });
  } }],
});
await server.listen();
console.log(`PR fixture preview: http://127.0.0.1:${server.config.server.port}/tests/pull-request-preview.html`);
console.log(`Mobile app preview: http://127.0.0.1:${server.config.server.port}/m`);
