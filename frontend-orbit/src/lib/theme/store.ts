import { themes } from './themes';
import { themeTokens } from './tokens';

export const THEME_STORAGE_KEY = 'orbit.theme';

function applyTheme(id: unknown) {
  const theme = themes.find(item => item.id === id) ?? themes[0];
  const root = document.documentElement;
  root.dataset.theme = theme.id;
  root.dataset.appearance = theme.appearance;
  root.style.colorScheme = theme.appearance;
  for (const [name, value] of Object.entries(themeTokens(theme))) root.style.setProperty(name, value);
}

/** Independent copy of AoW's palettes and token mapping, applied before login. */
export function initializeTheme() {
  let saved: string | null = null;
  try { saved = localStorage.getItem(THEME_STORAGE_KEY); } catch { /* Storage may be disabled. */ }
  applyTheme(saved);
  window.addEventListener('storage', event => {
    if (event.key === THEME_STORAGE_KEY || event.key === null) applyTheme(event.newValue);
  });
}
