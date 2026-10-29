import type { IncomingMessage, ServerResponse } from 'node:http';
import { HttpError } from './errors.js';

export function json(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store' });
  response.end(JSON.stringify(value));
}

export async function body(request: IncomingMessage, limit = 16_384): Promise<Record<string, unknown>> {
  if (!request.headers['content-type']?.toLowerCase().startsWith('application/json')) throw new HttpError(415, 'invalid_content_type', '请使用 JSON 格式提交请求。');
  const content = await new Promise<string>((resolveBody, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    request.on('data', (chunk: Buffer) => {
      size += chunk.length;
      if (size > limit) reject(new HttpError(413, 'body_too_large', '请求内容过大。'));
      else chunks.push(chunk);
    });
    request.on('end', () => resolveBody(Buffer.concat(chunks).toString('utf8')));
    request.on('error', reject);
    request.on('aborted', () => reject(new HttpError(400, 'request_aborted', '请求已中断。')));
  });
  try {
    const value: unknown = JSON.parse(content);
    if (value && typeof value === 'object' && !Array.isArray(value)) return value as Record<string, unknown>;
  } catch { /* Do not expose submitted credentials or file contents in errors. */ }
  throw new HttpError(400, 'invalid_json', '请求内容格式无效。');
}
