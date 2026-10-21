import { appLocalStorage, storageKey } from '../basePath';
import { themes } from './themes';
import { themeTokens } from './tokens';
import { applyTerminalPalette, initializeTerminalPalette } from './terminalPalette';

export const THEME_STORAGE_KEY = 'aow.theme';
export const defaultTheme = themes[0];
export const resolveTheme = (id: unknown) => themes.find(theme => theme.id === id) ?? defaultTheme;
const listeners = new Set<() => void>();
let current = defaultTheme;
let initialized = false;

export const getTheme = () => current;
export function subscribeTheme(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

function applyTheme(id: unknown) {
  current = resolveTheme(id);
  const root = document.documentElement;
  root.dataset.theme = current.id;
  root.dataset.appearance = current.appearance;
  root.style.colorScheme = current.appearance;
  for (const [name, value] of Object.entries(themeTokens(current))) root.style.setProperty(name, value);
  applyTerminalPalette(current.appearance);
  for (const listener of listeners) listener();
}

/** Apply before React renders, including login, mobile and shared-session pages. */
export function initializeTheme() {
  if (initialized) return;
  initialized = true;
  initializeTerminalPalette();
  let saved: string | null = null;
  try { saved = appLocalStorage.getItem(THEME_STORAGE_KEY); } catch { /* Storage can be disabled. */ }
  applyTheme(saved);
  window.addEventListener('storage', event => {
    if (event.storageArea !== window.localStorage) return;
    if (event.key === storageKey(THEME_STORAGE_KEY) || event.key === null) applyTheme(event.newValue);
  });
}

/** Changes are local to this browser/deployment and never restart a session. */
export function setTheme(id: string): boolean {
  applyTheme(id);
  try {
    appLocalStorage.setItem(THEME_STORAGE_KEY, current.id);
    return true;
  } catch {
    return false;
  }
}
