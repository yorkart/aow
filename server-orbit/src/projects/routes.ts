import type { IncomingMessage, ServerResponse } from 'node:http';
import { HttpError } from '../errors.js';
import { body, json } from '../requests.js';
import type { Projects } from './service.js';
import type { Workspaces } from './workspaces.js';
import type { Tasks } from './tasks.js';

export class ProjectRoutes {
  constructor(private projects: Projects, private workspaces: Workspaces, private tasks: Tasks) {}
  async handle(request: IncomingMessage, response: ServerResponse, url: URL): Promise<boolean> {
    const match = /^\/api\/orgs\/([A-Za-z0-9-]+)\/(projects|tasks)(?:\/([A-Za-z0-9-]+))?(?:\/(workspaces|tasks))?$/.exec(url.pathname);
    if (!match) return false;
    const [, org, resource, id, section] = match; const method = request.method;
    this.projects.checkOrg(org);
    if (resource === 'tasks' && id && !section) {
      if (method === 'GET') { json(response, 200, this.tasks.get(org, id)); return true; }
      if (method === 'PUT') { json(response, 200, await this.tasks.save(org, await body(request, 128 * 1024), id)); return true; }
    }
    if (resource === 'projects') {
      if (id === 'inspect' && !section && method === 'POST') { json(response, 200, await this.projects.inspect((await body(request)).directory)); return true; }
      if (id === 'directories' && !section && method === 'GET') { json(response, 200, await this.projects.directories(url.searchParams.get('path'))); return true; }
      if (!id && method === 'GET') { json(response, 200, this.projects.list(org)); return true; }
      if (!id && method === 'POST') {
        const input = await body(request, 64 * 1024);
        json(response, 201, input.codebase_path === undefined && typeof input.directory === 'string' ? await this.projects.register(org, input) : await this.projects.save(org, input)); return true;
      }
      if (id && !section && method === 'GET') { json(response, 200, this.projects.get(org, id)); return true; }
      if (id && !section && method === 'PUT') { json(response, 200, await this.projects.save(org, await body(request, 64 * 1024), id)); return true; }
      if (id && section === 'workspaces' && method === 'GET') { json(response, 200, await this.workspaces.list(org, id)); return true; }
      if (id && section === 'tasks') {
        this.projects.row(org, id);
        if (method === 'GET') { json(response, 200, this.tasks.list(org, id, url.searchParams.get('query') ?? '', url.searchParams.get('status') ?? '')); return true; }
        if (method === 'POST') { json(response, 201, await this.tasks.save(org, { ...await body(request, 128 * 1024), project_id: id })); return true; }
      }
    }
    throw new HttpError(405, 'method_not_allowed', '请求方法不支持。');
  }
}
