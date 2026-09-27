import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { preview } from 'vite';

// The production server injects the deployment base before serving index.html.
// Vite preview alone leaves relative assets broken when a deep link is reloaded.
export async function productionPreview(options = {}) {
  const root = fileURLToPath(new URL('../../', import.meta.url));
  const html = (await readFile(new URL('../../dist/index.html', import.meta.url), 'utf8'))
    .replace('<head>', '<head><base href="/"><meta name="aow-base-path" content="">');
  return preview({
    root,
    ...options,
    preview: { host: '127.0.0.1', port: 0, ...options.preview },
    plugins: [...options.plugins ?? [], {
      name: 'production-html-base',
      configurePreviewServer(server) {
        server.middlewares.use((request, response, next) => {
          if (!['GET', 'HEAD'].includes(request.method) || !request.headers.accept?.includes('text/html')
            || request.url.startsWith('/api/')) return next();
          response.setHeader('Content-Type', 'text/html; charset=utf-8');
          response.end(request.method === 'HEAD' ? undefined : html);
        });
      },
    }],
  });
}
