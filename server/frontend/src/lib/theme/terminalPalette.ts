import { appLocalStorage, storageKey } from '../basePath';
import type { Theme } from './types';

export interface TerminalPalette {
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

export type TerminalPalettePreference = 'auto' | 'soft-dark' | 'soft-light';
export const TERMINAL_PALETTE_STORAGE_KEY = 'aow.terminal-palette';

// Restore the pre-theme terminal: Tango ANSI colors with AOW's background and selection.
export const terminalPalettes: Record<'soft-dark' | 'soft-light', TerminalPalette> = {
  'soft-dark': {
    background: '#1e1e1e', foreground: '#cccccc', cursor: '#ffffff', selectionBackground: '#264f78',
    black: '#000000', red: '#cc0000', green: '#4e9a06', yellow: '#c4a000',
    blue: '#3465a4', magenta: '#75507b', cyan: '#06989a', white: '#d3d7cf',
    brightBlack: '#666666', brightRed: '#ef2929', brightGreen: '#8ae234', brightYellow: '#fce94f',
    brightBlue: '#729fcf', brightMagenta: '#ad7fa8', brightCyan: '#34e2e2', brightWhite: '#eeeeec',
  },
  'soft-light': {
    background: '#fafafa', foreground: '#383a42', cursor: '#383a42', selectionBackground: '#d6e4f2',
    black: '#383a42', red: '#a34343', green: '#52733f', yellow: '#84651d',
    blue: '#496f9d', magenta: '#83547f', cyan: '#35777c', white: '#6b7078',
    brightBlack: '#717780', brightRed: '#b34f4f', brightGreen: '#5b7d45', brightYellow: '#8d6d24',
    brightBlue: '#567da8', brightMagenta: '#92618e', brightCyan: '#408187', brightWhite: '#555b63',
  },
};

const resolvePreference = (value: unknown): TerminalPalettePreference =>
  value === 'soft-dark' || value === 'soft-light' ? value : 'auto';
const listeners = new Set<() => void>();
let current: TerminalPalettePreference = 'auto';
let initialized = false;
let appearance: Theme['appearance'] = 'dark';

export function applyTerminalPalette(nextAppearance = appearance) {
  appearance = nextAppearance;
  const palette = resolveTerminalPalette(appearance);
  const root = document.documentElement;
  root.dataset.terminalPalette = current === 'auto' ? `soft-${appearance}` : current;
  root.style.setProperty('--aow-terminal-background', palette.background);
  root.style.setProperty('--aow-terminal-foreground', palette.foreground);
}

function applyPreference(value: unknown) {
  current = resolvePreference(value);
  applyTerminalPalette();
  for (const listener of listeners) listener();
}

export function initializeTerminalPalette() {
  if (initialized) return;
  initialized = true;
  try { current = resolvePreference(appLocalStorage.getItem(TERMINAL_PALETTE_STORAGE_KEY)); } catch { /* Storage may be disabled. */ }
  window.addEventListener('storage', event => {
    if (event.storageArea !== window.localStorage) return;
    if (event.key === storageKey(TERMINAL_PALETTE_STORAGE_KEY) || event.key === null) applyPreference(event.newValue);
  });
}

export const getTerminalPalettePreference = () => current;
export function subscribeTerminalPalette(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

export function resolveTerminalPalette(appearance: Theme['appearance'], preference = current) {
  return terminalPalettes[preference === 'auto' ? (appearance === 'light' ? 'soft-light' : 'soft-dark') : preference];
}

/** Persist separately from the workbench theme; applying never recreates a terminal. */
export function setTerminalPalettePreference(preference: TerminalPalettePreference): boolean {
  applyPreference(preference);
  try {
    appLocalStorage.setItem(TERMINAL_PALETTE_STORAGE_KEY, current);
    return true;
  } catch {
    return false;
  }
}
