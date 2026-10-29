import type { IncomingMessage, ServerResponse } from 'node:http';
import { HttpError } from '../errors.js';
import { body, json } from '../requests.js';
import type { Agents } from './service.js';

export class AgentRoutes {
  constructor(private agents: Agents) {}
  async handle(request: IncomingMessage, response: ServerResponse, url: URL): Promise<boolean> {
    const match = /^\/api\/orgs\/([A-Za-z0-9-]+)\/agents(?:\/([A-Za-z0-9-]+))?(?:\/(identity|instructions|skills|runtime|variables|check))?$/.exec(url.pathname);
    if (!match) return false;
    const [, org, id, section] = match; const method = request.method;
    this.agents.connectors.checkOrg(org);
    if (id === 'runtime-options' && !section && method === 'GET') { json(response, 200, await this.agents.runtimeOptions(org, url.searchParams.get('connector_id') ?? '')); return true; }
    if (!id && method === 'GET') { json(response, 200, this.agents.list(org)); return true; }
    if (!id && method === 'POST') { json(response, 201, await this.agents.create(org, await body(request))); return true; }
    if (id) {
      if (!section && method === 'GET') { json(response, 200, this.agents.get(org, id)); return true; }
      if (section === 'instructions' && method === 'GET') { json(response, 200, await this.agents.instructionFiles(org, id)); return true; }
      if (section === 'check' && method === 'POST') { json(response, 200, await this.agents.checkRuntime(org, id, await body(request))); return true; }
      if (section && method === 'PUT') {
        const input = await body(request, section === 'instructions' ? 8 * 1024 * 1024 : section === 'variables' ? 512 * 1024 : 16_384);
        const result = section === 'identity' ? this.agents.saveIdentity(org, id, input) : section === 'instructions' ? await this.agents.saveInstructions(org, id, input) : section === 'skills' ? this.agents.saveSkills(org, id, input) : section === 'runtime' ? await this.agents.saveRuntime(org, id, input) : section === 'variables' ? this.agents.saveVariables(org, id, input) : undefined;
        if (result) { json(response, 200, result); return true; }
      }
    }
    throw new HttpError(405, 'method_not_allowed', '请求方法不支持。');
  }
}
