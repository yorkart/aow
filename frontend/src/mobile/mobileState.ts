import { appSessionStorage, appUrl } from '../lib/basePath';
import { tabLocationUrl, type ResolvedTab } from '../aow/tabRoutes';
import { mobileRouteForTab, mobileTabTarget } from '../aow/tabRoutes/mobile';
import type { DocumentSource } from '../features/editor/types';
import type { AowProject } from '../aow/types';
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';

export type MobileNavigate = (route: Partial<MobileRoute>, replace?: boolean) => void;

export type MobileView = 'terminal' | 'sessions' | 'files' | 'git' | 'pull-requests' | 'automations';
export interface MobileRoute {
  workspace: string;
  view: MobileView;
  directory?: string;
  file?: string;
  notes?: string;
  session?: string;
  agent?: string;
  diff?: string;
  staged?: string;
  task?: string;
  run?: string;
  pr?: string;
  terminal?: string;
  fileSource?: DocumentSource;
  cwd?: string;
  repository?: string;
  provider?: string;
  remote?: string;
  commit?: string;
  originalPath?: string;
  untracked?: string;
  automationView?: string;
}

export function readMobileRoute(hash = window.location.hash): MobileRoute {
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  const view = params.get('view');
  return {
    ...Object.fromEntries(params), workspace: params.get('workspace') ?? '',
    view: ['terminal', 'sessions', 'files', 'git', 'pull-requests', 'automations'].includes(view ?? '') ? view as MobileView : 'terminal',
  };
}

export function mobileRouteUrl(route: Partial<MobileRoute>) {
  const params = new URLSearchParams();
  Object.entries(route).forEach(([key, value]) => { if (value) params.set(key, value); });
  return `#${params}`;
}

export function useMobileRoute(initial?: ResolvedTab, projects: AowProject[] = []) {
  const [route, setRoute] = useState(() => initial ? mobileRouteForTab(initial) : readMobileRoute());
  useEffect(() => {
    if (initial) setRoute(mobileRouteForTab(initial));
  }, [initial]);
  useEffect(() => {
    const update = () => { if (!window.location.pathname.startsWith(appUrl('/aow/tabs/'))) setRoute(readMobileRoute()); };
    window.addEventListener('popstate', update);
    window.addEventListener('hashchange', update);
    return () => { window.removeEventListener('popstate', update); window.removeEventListener('hashchange', update); };
  }, []);
  const navigate = useCallback((next: Partial<MobileRoute>, replace = false) => {
    const hash = mobileRouteUrl(next);
    const target = mobileTabTarget(next, projects);
    const url = tabLocationUrl(target, target ? '' : hash);
    if (window.location.pathname + window.location.search + window.location.hash === url) return;
    const depth = Number(window.history.state?.aowMobileDepth) || 0;
    window.history[replace ? 'replaceState' : 'pushState']({ ...window.history.state, aowMobileDepth: replace ? depth : depth + 1 }, '', url);
    setRoute({ workspace: '', view: 'terminal', ...next });
  }, [projects]);
  const back = useCallback((fallback: Partial<MobileRoute>) => {
    if (window.history.state?.aowMobileDepth > 0) window.history.back();
    else navigate(fallback, true);
  }, [navigate]);
  return { route, navigate, back };
}

export function useMobileResource<T>(key: string, loader: () => Promise<T>, enabled = true) {
  const loaderRef = useRef(loader);
  loaderRef.current = loader;
  const [revision, setRevision] = useState(0);
  const [state, setState] = useState<{ key: string; data?: T; error?: string; loading: boolean }>({ key, loading: enabled });
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setState((old) => ({ key, data: old.key === key ? old.data : undefined, loading: true }));
    Promise.resolve().then(() => loaderRef.current()).then(
      (data) => { if (!cancelled) setState({ key, data, loading: false }); },
      (error: unknown) => { if (!cancelled) setState((old) => ({ ...old, key, error: error instanceof Error ? error.message : String(error), loading: false })); },
    );
    return () => { cancelled = true; };
  }, [key, revision, enabled]);
  const reload = useCallback(() => setRevision((value) => value + 1), []);
  return { data: state.key === key ? state.data : undefined, error: state.key === key ? state.error : undefined,
    loading: state.key === key ? state.loading : enabled, reload };
}

export function storedMobileValue(key: string, fallback = '') {
  try { return appSessionStorage.getItem(`aow.mobile.${key}`) ?? fallback; } catch { return fallback; }
}
export function saveMobileValue(key: string, value: string) {
  try { appSessionStorage.setItem(`aow.mobile.${key}`, value); } catch { /* Storage may be disabled by the host. */ }
}

export function useMobileScroll(key: string, ready = true) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!node || !ready) return;
    node.scrollTop = Number(storedMobileValue(`scroll.${key}`, '0')) || 0;
    const save = () => saveMobileValue(`scroll.${key}`, String(node.scrollTop));
    node.addEventListener('scroll', save, { passive: true });
    return () => node.removeEventListener('scroll', save);
  }, [key, ready]);
  return ref;
}

declare global {
  interface Window {
    aowHost?: { setTitle?: (title: string) => void; close?: () => void };
  }
}

export function useMobileViewport(title: string) {
  useEffect(() => {
    document.title = `${title} · AoW`;
    window.aowHost?.setTitle?.(title);
  }, [title]);
  useEffect(() => {
    const viewport = window.visualViewport;
    const root = document.documentElement;
    let frame = 0;
    let settled = 0;
    let closing = 0;
    let layoutHeight = 0;
    let keyboardSession = false;
    const update = (finishClosing = false) => {
      // Some WebViews update innerHeight/dvh before the visual viewport. Use
      // visual height directly unless its width still belongs to the old orientation.
      const staleOrientation = viewport && Math.abs(viewport.width * (viewport.scale || 1) - window.innerWidth) > 2;
      const height = Math.max(1, !viewport || staleOrientation ? window.innerHeight : viewport.height);
      const top = Math.max(0, staleOrientation ? 0 : viewport?.offsetTop ?? 0);
      const focused = document.activeElement?.matches('input, textarea, [contenteditable="true"]') ?? false;
      if (!layoutHeight) layoutHeight = height;
      if (focused) {
        keyboardSession = true;
        clearTimeout(closing);
        closing = 0;
      } else if (keyboardSession && (height >= layoutHeight - 1 || finishClosing)) {
        keyboardSession = false;
        clearTimeout(closing);
        closing = 0;
      } else if (keyboardSession && !closing) {
        // Keep the old terminal height through the keyboard's closing animation.
        closing = window.setTimeout(() => update(true), 1000);
      }
      if (!keyboardSession) layoutHeight = height;
      root.toggleAttribute('data-mobile-keyboard', keyboardSession && height < layoutHeight - 1);
      root.style.setProperty('--mobile-height', `${height}px`);
      root.style.setProperty('--mobile-top', `${top}px`);
      root.style.setProperty('--mobile-keyboard-inset', `${Math.max(0, layoutHeight - height)}px`);
      // The app scrolls only inside its own panes. Undo iOS focus-induced root
      // scrolling, which can otherwise expose a blank strip below the app.
      if (window.scrollX || window.scrollY) window.scrollTo(0, 0);
    };
    const schedule = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => update());
    };
    const settle = () => {
      update();
      schedule();
      clearTimeout(settled);
      settled = window.setTimeout(schedule, 300);
    };
    update();
    viewport?.addEventListener('resize', schedule);
    viewport?.addEventListener('scroll', schedule);
    window.addEventListener('resize', settle);
    window.addEventListener('scroll', schedule);
    window.addEventListener('orientationchange', settle);
    document.addEventListener('focusin', settle);
    document.addEventListener('focusout', settle);
    return () => {
      cancelAnimationFrame(frame);
      clearTimeout(settled);
      clearTimeout(closing);
      viewport?.removeEventListener('resize', schedule);
      viewport?.removeEventListener('scroll', schedule);
      window.removeEventListener('resize', settle);
      window.removeEventListener('scroll', schedule);
      window.removeEventListener('orientationchange', settle);
      document.removeEventListener('focusin', settle);
      document.removeEventListener('focusout', settle);
      root.removeAttribute('data-mobile-keyboard');
      for (const name of ['--mobile-height', '--mobile-top', '--mobile-keyboard-inset']) root.style.removeProperty(name);
    };
  }, []);
}
