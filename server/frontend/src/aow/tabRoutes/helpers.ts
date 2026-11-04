import { t } from '../../lib/i18n';
import { appUrl } from '../../lib/basePath';
export const prefix = appUrl('/aow/tabs/');
export function required(query: URLSearchParams, key: string) {
  const value = query.get(key);
  if (!value || value.includes('\0')) throw new Error(t("aow.tab_link_is_missing_a_valid_parameter", { value0: key }));
  return value;
}
export function absolutePath(value: string) {
  if (!value.startsWith('/') || value.includes('\0')) throw new Error(t("aow.paths_in_tab_links_must_be_absolute"));
  return value;
}
export function segment(segments: string[], index: number) {
  const value = segments[index];
  if (!value || value.includes('\0')) throw new Error(t("aow.tab_link_is_missing_a_resource_identifier"));
  return value;
}
export function length(segments: string[], expected: number) {
  if (segments.length !== expected) throw new Error(t("aow.unrecognized_tab_link"));
}
export function url(path: string, params: Record<string, string | undefined>) {
  const query = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => { if (value !== undefined) query.set(key, value); });
  return prefix + path + (query.size ? '?' + query : '');
}

