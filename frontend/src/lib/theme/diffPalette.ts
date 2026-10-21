import { appLocalStorage, storageKey } from '../basePath';
import type { Theme } from './types';

export type DiffPalettePreference = 'auto' | 'dark' | 'light';
export const DIFF_PALETTE_STORAGE_KEY = 'aow.diff-palette';

// Restore Monaco's original vs-dark/vs diff colors independently of UI status colors.
export const diffPalettes = {
  dark: {
    insertedText: '#9ccc2c33', removedText: '#ff000033',
    insertedLine: '#9bb95533', removedLine: '#ff000033',
    insertedGutter: '#9bb95533', removedGutter: '#ff000033',
    insertedOverview: '#9ccc2ccc', removedOverview: '#ff0000cc',
  },
  light: {
    insertedText: '#9ccc2c40', removedText: '#ff000033',
    insertedLine: '#9bb95533', removedLine: '#ff000033',
    insertedGutter: '#9bb95533', removedGutter: '#ff000033',
    insertedOverview: '#668c20cc', removedOverview: '#c02020cc',
  },
};

const resolvePreference = (value: unknown): DiffPalettePreference =>
  value === 'dark' || value === 'light' ? value : 'auto';
const listeners = new Set<() => void>();
let current: DiffPalettePreference = 'auto';
let initialized = false;
let appearance: Theme['appearance'] = 'dark';

export function applyDiffPalette(nextAppearance = appearance) {
  appearance = nextAppearance;
  const palette = resolveDiffPalette(appearance);
  const root = document.documentElement;
  root.dataset.diffPalette = current === 'auto' ? appearance : current;
  root.style.setProperty('--aow-diff-inserted-background', palette.insertedLine);
  root.style.setProperty('--aow-diff-removed-background', palette.removedLine);
}

function applyPreference(value: unknown) {
  current = resolvePreference(value);
  applyDiffPalette();
  for (const listener of listeners) listener();
}

export function initializeDiffPalette() {
  if (initialized) return;
  initialized = true;
  let saved: string | null = null;
  try { saved = appLocalStorage.getItem(DIFF_PALETTE_STORAGE_KEY); } catch { /* Storage may be disabled. */ }
  applyPreference(saved);
  window.addEventListener('storage', event => {
    if (event.storageArea !== window.localStorage) return;
    if (event.key === storageKey(DIFF_PALETTE_STORAGE_KEY) || event.key === null) applyPreference(event.newValue);
  });
}

export const getDiffPalettePreference = () => current;
export function subscribeDiffPalette(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function resolveDiffPalette(appearance: Theme['appearance'], preference = current) {
  return diffPalettes[preference === 'auto' ? (appearance === 'light' ? 'light' : 'dark') : preference];
}

/** Persist independently; subscribers update existing diff views in place. */
export function setDiffPalettePreference(preference: DiffPalettePreference): boolean {
  applyPreference(preference);
  try {
    appLocalStorage.setItem(DIFF_PALETTE_STORAGE_KEY, current);
    return true;
  } catch {
    return false;
  }
}
