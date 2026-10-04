import { appLocalStorage } from '../../lib/basePath';
export function readDraft<T>(key: string): T | undefined {
  try { return JSON.parse(appLocalStorage.getItem(`aow-inbox-draft:${key}`) ?? 'null') ?? undefined; } catch { return; }
}
export function saveDraft(key: string, value?: unknown) {
  try {
    if (value === undefined) appLocalStorage.removeItem(`aow-inbox-draft:${key}`);
    else appLocalStorage.setItem(`aow-inbox-draft:${key}`, JSON.stringify(value));
  } catch { /* Draft remains in memory when browser storage is unavailable. */ }
}
