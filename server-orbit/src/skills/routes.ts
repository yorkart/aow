import type { IncomingMessage, ServerResponse } from 'node:http';
import { HttpError } from '../errors.js';
import { body, json } from '../requests.js';
import { SkillProjects } from './projects.js';
import type { Skills } from './service.js';

export class SkillRoutes {
  private projects: SkillProjects;
  constructor(private skills: Skills) { this.projects = new SkillProjects(skills); }

  async handle(request: IncomingMessage, response: ServerResponse, url: URL): Promise<boolean> {
    const match = /^\/api\/orgs\/([a-zA-Z0-9-]+)\/(skills)(?:\/([a-zA-Z0-9-]+))?$/.exec(url.pathname);
    if (!match) return false;
    const [, org, , id] = match;
    this.skills.checkOrg(org);
    const method = request.method;
    if (!id) {
      if (method === 'GET') { json(response, 200, this.skills.list(org, url.searchParams.get('query') ?? '')); return true; }
      if (method === 'POST') { json(response, 201, await this.skills.create(org, await body(request, 3 * 1024 * 1024))); return true; }
    } else if (id === 'scan' && method === 'POST') {
      json(response, 200, await this.projects.scan(org, (await body(request)).project_id)); return true;
    } else if (id === 'import' && method === 'POST') {
      json(response, 201, await this.projects.import(org, await body(request, 128 * 1024))); return true;
    } else {
      if (method === 'GET') { json(response, 200, await this.skills.detail(org, id)); return true; }
      if (method === 'PUT') { json(response, 200, await this.skills.save(org, id, await body(request, 32 * 1024 * 1024))); return true; }
      if (method === 'DELETE') { this.skills.remove(org, id); response.writeHead(204); response.end(); return true; }
    }
    throw new HttpError(405, 'method_not_allowed', '请求方法不支持。');
  }
}
