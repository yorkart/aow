import { appUrl } from '../../lib/basePath';
export const prefix = appUrl('/aow/tabs/');
export function required(query: URLSearchParams, key: string) {
  const value = query.get(key);
  if (!value || value.includes('\0')) throw new Error(`Tab 链接缺少有效的 ${key} 参数。`);
  return value;
}
export function absolutePath(value: string) {
  if (!value.startsWith('/') || value.includes('\0')) throw new Error('Tab 链接中的路径必须为绝对路径。');
  return value;
}
export function segment(segments: string[], index: number) {
  const value = segments[index];
  if (!value || value.includes('\0')) throw new Error('Tab 链接缺少资源标识。');
  return value;
}
export function length(segments: string[], expected: number) {
  if (segments.length !== expected) throw new Error('无法识别此 Tab 链接。');
}
export function url(path: string, params: Record<string, string | undefined>) {
  const query = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => { if (value !== undefined) query.set(key, value); });
  return prefix + path + (query.size ? '?' + query : '');
}

