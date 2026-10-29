import type { IncomingMessage, ServerResponse } from 'node:http';
import { HttpError } from '../errors.js';
import { body, json } from '../requests.js';
import type { Connectors } from './service.js';

export class ConnectorRoutes {
  constructor(private connectors: Connectors) {}
  async handle(request: IncomingMessage, response: ServerResponse, url: URL): Promise<boolean> {
    const match = /^\/api\/orgs\/([a-zA-Z0-9-]+)\/(connectors|connector-auth)(?:\/([a-zA-Z0-9-]+))?(?:\/(check|enabled|code))?$/.exec(url.pathname);
    if (!match) return false;
    const [, org, resource, id, action] = match;
    this.connectors.checkOrg(org);
    const method = request.method;
    if (resource === 'connector-auth') {
      if (!id && method === 'POST') {
        const input = await body(request);
        const selected = await this.connectors.adapters.validate(input.provider, input.executor, 'independent');
        json(response, 201, await this.connectors.authFlows.start(org, selected.provider, selected.executor)); return true;
      }
      if (id && !action && method === 'GET') { json(response, 200, this.connectors.authFlows.publicStatus(org, id)); return true; }
      if (id && action === 'code' && method === 'POST') { json(response, 200, await this.connectors.authFlows.submit(org, id, (await body(request)).code)); return true; }
      if (id && !action && method === 'DELETE') { json(response, 200, this.connectors.authFlows.cancel(org, id)); return true; }
    } else if (id === 'catalog' && method === 'GET') { json(response, 200, await this.connectors.adapters.catalog()); return true;
    } else if (id === 'verify' && method === 'POST') { json(response, 200, await this.connectors.verify(org, await body(request, 32 * 1024))); return true;
    } else if (!id && method === 'GET') { json(response, 200, this.connectors.list(org)); return true;
    } else if (!id && method === 'POST') { json(response, 201, await this.connectors.save(org, await body(request, 32 * 1024))); return true;
    } else if (id && action === 'check' && method === 'POST') { json(response, 200, await this.connectors.check(org, id)); return true;
    } else if (id && action === 'enabled' && method === 'PUT') { json(response, 200, this.connectors.enable(org, id, await body(request))); return true;
    } else if (id && !action && method === 'GET') { json(response, 200, this.connectors.get(org, id)); return true;
    } else if (id && !action && method === 'PUT') { json(response, 200, await this.connectors.save(org, await body(request, 32 * 1024), id)); return true;
    } else if (id && !action && method === 'DELETE') { this.connectors.remove(org, id, Number(url.searchParams.get('revision'))); response.writeHead(204); response.end(); return true; }
    throw new HttpError(405, 'method_not_allowed', '请求方法不支持。');
  }
}
